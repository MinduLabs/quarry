//! Schema compare: what to change in a target database so its tables match a source database,
//! as `SchemaChange`s the target's driver turns into a script. Applying it is reviewed like any
//! schema change.

use std::collections::BTreeMap;
use std::time::Instant;

use quarry_model::{
    AlterOp, ColumnDef, ColumnInfo, ConnId, ForeignKey, IndexDef, IndexInfo, ObjectKind,
    SchemaChange, SchemaDiff, SchemaSide, TableDiff, TableDiffKind, TableInfo, TableRef,
};

use crate::engine::{Engine, describe};
use crate::{Event, RequestId, schema};

impl Engine {
    pub(crate) async fn compare_schemas(
        &self,
        request: RequestId,
        source: SchemaSide,
        target: SchemaSide,
    ) {
        let started = Instant::now();
        let result = self.schema_diff(source, target).await;
        let tables = result.as_ref().map(|d| d.tables.len()).ok();
        self.perf(
            "schema.compare",
            started,
            serde_json::json!({ "differing": tables }),
        );
        let (diff, error) = match result {
            Ok(diff) => (Some(diff), None),
            Err(error) => (None, Some(error)),
        };
        self.emit(Event::SchemaCompared {
            request,
            diff,
            error,
        });
    }

    async fn schema_diff(
        &self,
        source: SchemaSide,
        target: SchemaSide,
    ) -> Result<SchemaDiff, String> {
        let config = |side: &SchemaSide| {
            self.config_of(side.conn)
                .ok_or_else(|| "Connection not found".to_owned())
        };
        if config(&source)?.driver != config(&target)?.driver {
            return Err(
                "Compare two databases of the same kind: their SQL types and DDL differ."
                    .to_owned(),
            );
        }
        let source_tables = self.tables_of(&source).await?;
        let target_tables = self.tables_of(&target).await?;
        let diff = diff_tables(
            &source.database,
            &target.database,
            &source_tables,
            &target_tables,
        );
        let driver = self.driver_of(target.conn)?;
        let mut statements = Vec::new();
        for change in &diff.changes {
            statements.extend(schema::checked(driver.schema_change(change).await)?);
        }
        Ok(SchemaDiff {
            source,
            target,
            tables: diff.tables,
            same: diff.same,
            changes: diff.changes,
            statements,
        })
    }

    /// Every table of `side` with its columns, keys and indexes. Views are left out.
    async fn tables_of(&self, side: &SchemaSide) -> Result<Vec<TableInfo>, String> {
        let meta = self.meta_session(side.conn).await?;
        let objects = self
            .with_meta(side.conn, meta, |s| {
                let database = side.database.clone();
                Box::pin(async move { s.list_objects(&database).await })
            })
            .await
            .map_err(|e| describe(&e))?;
        let mut tables = Vec::new();
        for object in objects.into_iter().filter(|o| o.kind == ObjectKind::Table) {
            let table = TableRef {
                database: Some(side.database.clone()),
                schema: None,
                name: object.name,
            };
            tables.push(self.table_info_of(side.conn, &table).await?);
        }
        Ok(tables)
    }

    /// Review (`confirmed: false`) or run the changes of a comparison on `conn`. The statements
    /// are rebuilt here from `changes`, never taken from the client.
    pub(crate) async fn apply_schema_diff(
        &self,
        request: RequestId,
        conn: ConnId,
        changes: Vec<SchemaChange>,
        confirmed: bool,
    ) {
        let done = |applied: u64, error: Option<String>| Event::SchemaDiffApplied {
            request,
            conn,
            applied,
            error,
        };
        let Some(config) = self.config_of(conn) else {
            return self.emit(done(0, Some("Connection not found".into())));
        };
        if config.read_only {
            return self.emit(done(0, Some("This connection is read-only.".into())));
        }
        if changes.is_empty() {
            return self.emit(done(0, Some("The databases already match.".into())));
        }
        let driver = match self.driver_of(conn) {
            Ok(driver) => driver,
            Err(error) => return self.emit(done(0, Some(error))),
        };
        let mut statements = Vec::new();
        for change in &changes {
            match schema::checked(driver.schema_change(change).await) {
                Ok(written) => statements.extend(written),
                Err(error) => return self.emit(done(0, Some(error))),
            }
        }
        if !confirmed {
            return self.emit(Event::SchemaDiffReview {
                request,
                conn,
                statements,
                warning: schema::warning_for_all(config.environment, &changes),
            });
        }
        let database = changes.iter().find_map(SchemaChange::database);
        let ran = self.run_ddl(conn, database, statements, false).await;
        self.emit(done(ran.applied, ran.error));
    }
}

/// What `diff_tables` found: the tables that differ, how many are the same, and the changes that
/// make the target match, in an order they can run in.
pub(crate) struct Diff {
    pub tables: Vec<TableDiff>,
    pub same: u32,
    pub changes: Vec<SchemaChange>,
}

/// How one table of the target changes, by phase: foreign keys go first (a dropped column or index
/// may hold one) and come back last (a new one may point at a table created on the way).
#[derive(Default)]
struct TableChanges {
    details: Vec<String>,
    drop_keys: Vec<AlterOp>,
    middle: Vec<SchemaChange>,
    add_keys: Vec<AlterOp>,
}

/// Compare `source` (tables of `source_db`) with `target` (of `target_db`), by table name.
pub(crate) fn diff_tables(
    source_db: &str,
    target_db: &str,
    source: &[TableInfo],
    target: &[TableInfo],
) -> Diff {
    let by_name = |tables: &[TableInfo]| -> BTreeMap<String, TableInfo> {
        tables
            .iter()
            .map(|t| (t.table.name.clone(), t.clone()))
            .collect()
    };
    let source = by_name(source);
    let target = by_name(target);
    let names: std::collections::BTreeSet<&String> = source.keys().chain(target.keys()).collect();
    let mut tables = Vec::new();
    let mut same = 0;
    let (mut first, mut middle, mut last) = (Vec::new(), Vec::new(), Vec::new());
    for name in names {
        let table = TableRef {
            database: Some(target_db.to_owned()),
            schema: None,
            name: name.clone(),
        };
        let (kind, found) = match (source.get(name), target.get(name)) {
            (Some(src), None) => (
                TableDiffKind::Missing,
                missing_table(&table, &retarget(src, source_db, target_db)),
            ),
            (Some(src), Some(dst)) => (
                TableDiffKind::Changed,
                changed_table(&table, &retarget(src, source_db, target_db), dst),
            ),
            (None, Some(dst)) => (TableDiffKind::Extra, extra_table(&table, dst)),
            (None, None) => continue,
        };
        if found.details.is_empty() {
            same += 1;
            continue;
        }
        let alter = |ops: Vec<AlterOp>| {
            (!ops.is_empty()).then(|| SchemaChange::AlterTable {
                table: table.clone(),
                ops,
            })
        };
        first.extend(alter(found.drop_keys));
        middle.extend(found.middle);
        last.extend(alter(found.add_keys));
        tables.push(TableDiff {
            name: name.clone(),
            kind,
            details: found.details,
        });
    }
    Diff {
        tables,
        same,
        changes: [first, middle, last].concat(),
    }
}

/// `info` with its foreign keys into its own database pointed at `target_db`: the copy refers to
/// the target's tables, not back to the source.
fn retarget(info: &TableInfo, source_db: &str, target_db: &str) -> TableInfo {
    let mut info = info.clone();
    for fk in &mut info.foreign_keys {
        if fk.references.database.as_deref() == Some(source_db) {
            fk.references.database = Some(target_db.to_owned());
        }
    }
    info
}

fn column_def(column: &ColumnInfo) -> ColumnDef {
    ColumnDef {
        name: column.name.clone(),
        type_name: column.type_name.clone(),
        nullable: column.nullable,
        default: column.default.clone(),
        auto_increment: column.auto_increment,
        on_update: column.on_update.clone(),
        comment: column.comment.clone(),
    }
}

fn index_def(index: &IndexInfo) -> IndexDef {
    IndexDef {
        name: index.name.clone(),
        columns: index.columns.clone(),
        unique: index.unique,
    }
}

fn describe_column(column: &ColumnInfo) -> String {
    let null = if column.nullable { "NULL" } else { "NOT NULL" };
    let default = column
        .default
        .as_deref()
        .map(|d| format!(" DEFAULT {d}"))
        .unwrap_or_default();
    format!("{} {} {null}{default}", column.name, column.type_name)
}

fn describe_index(index: &IndexInfo) -> String {
    let unique = if index.unique { "unique " } else { "" };
    format!(
        "{unique}index {} ({})",
        index.name,
        index.columns.join(", ")
    )
}

fn describe_key(fk: &ForeignKey) -> String {
    format!(
        "{} ({}) → {}({})",
        fk.name,
        fk.columns.join(", "),
        fk.references.name,
        fk.referenced_columns.join(", ")
    )
}

fn missing_table(table: &TableRef, src: &TableInfo) -> TableChanges {
    let mut found = TableChanges {
        details: vec![format!(
            "+ table {} ({} columns)",
            table.name,
            src.columns.len()
        )],
        ..TableChanges::default()
    };
    found.middle.push(SchemaChange::CreateTable {
        table: table.clone(),
        columns: src.columns.iter().map(column_def).collect(),
        primary_key: src.primary_key.clone(),
    });
    let indexes: Vec<AlterOp> = src
        .indexes
        .iter()
        .filter(|i| !i.primary)
        .map(|i| AlterOp::AddIndex {
            index: index_def(i),
        })
        .collect();
    if !indexes.is_empty() {
        found.middle.push(SchemaChange::AlterTable {
            table: table.clone(),
            ops: indexes,
        });
    }
    for fk in &src.foreign_keys {
        found
            .details
            .push(format!("+ foreign key {}", describe_key(fk)));
        found.add_keys.push(AlterOp::AddForeignKey {
            foreign_key: fk.clone(),
        });
    }
    found
}

/// A table only the target has. Its foreign keys go in the first phase, so tables that point at
/// each other can be dropped in any order.
fn extra_table(table: &TableRef, dst: &TableInfo) -> TableChanges {
    TableChanges {
        details: vec![format!("- table {}", table.name)],
        drop_keys: dst
            .foreign_keys
            .iter()
            .map(|fk| AlterOp::DropForeignKey {
                name: fk.name.clone(),
            })
            .collect(),
        middle: vec![SchemaChange::DropTable {
            table: table.clone(),
            view: false,
        }],
        ..TableChanges::default()
    }
}

/// What differs in one column, in words, or `None` when nothing does.
fn column_change(src: &ColumnInfo, dst: &ColumnInfo) -> Option<String> {
    let mut parts = Vec::new();
    if !src.type_name.eq_ignore_ascii_case(&dst.type_name) {
        parts.push(format!("{} → {}", dst.type_name, src.type_name));
    }
    if src.nullable != dst.nullable {
        parts.push(
            if src.nullable {
                "NOT NULL → NULL"
            } else {
                "NULL → NOT NULL"
            }
            .to_owned(),
        );
    }
    if src.default != dst.default {
        let show = |d: &Option<String>| d.clone().unwrap_or_else(|| "no default".to_owned());
        parts.push(format!(
            "default {} → {}",
            show(&dst.default),
            show(&src.default)
        ));
    }
    if src.auto_increment != dst.auto_increment {
        parts.push(format!(
            "auto increment {} → {}",
            dst.auto_increment, src.auto_increment
        ));
    }
    if src.on_update != dst.on_update {
        parts.push("on update".to_owned());
    }
    if src.comment != dst.comment {
        parts.push("comment".to_owned());
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Two foreign keys of the same name that point the same way with the same actions.
fn same_key(a: &ForeignKey, b: &ForeignKey) -> bool {
    let action = |v: &Option<String>| v.as_deref().map(str::to_uppercase);
    a.columns == b.columns
        && a.references.database == b.references.database
        && a.references.name == b.references.name
        && a.referenced_columns == b.referenced_columns
        && action(&a.on_update) == action(&b.on_update)
        && action(&a.on_delete) == action(&b.on_delete)
}

fn changed_table(table: &TableRef, src: &TableInfo, dst: &TableInfo) -> TableChanges {
    let mut found = TableChanges::default();
    let details = &mut found.details;
    let mut drop_indexes = Vec::new();
    let mut add_columns = Vec::new();
    let mut modify_columns = Vec::new();
    let mut drop_columns = Vec::new();
    let mut add_indexes = Vec::new();

    for column in &src.columns {
        match dst.columns.iter().find(|c| c.name == column.name) {
            None => {
                details.push(format!("+ column {}", describe_column(column)));
                if column.generated {
                    details.push(format!("  generated column {}: not scripted", column.name));
                } else {
                    add_columns.push(AlterOp::AddColumn {
                        column: column_def(column),
                    });
                }
            }
            Some(other) => {
                let Some(what) = column_change(column, other) else {
                    continue;
                };
                details.push(format!("~ column {}: {what}", column.name));
                if column.generated || other.generated {
                    details.push(format!("  generated column {}: not scripted", column.name));
                } else {
                    modify_columns.push(AlterOp::ModifyColumn {
                        name: column.name.clone(),
                        column: column_def(column),
                    });
                }
            }
        }
    }
    for column in dst
        .columns
        .iter()
        .filter(|c| !src.columns.iter().any(|s| s.name == c.name))
    {
        details.push(format!("- column {}", column.name));
        drop_columns.push(AlterOp::DropColumn {
            name: column.name.clone(),
        });
    }

    let secondary = |info: &TableInfo| -> Vec<IndexInfo> {
        info.indexes
            .iter()
            .filter(|i| !i.primary)
            .cloned()
            .collect()
    };
    let (src_indexes, dst_indexes) = (secondary(src), secondary(dst));
    for index in &src_indexes {
        match dst_indexes.iter().find(|i| i.name == index.name) {
            None => {
                details.push(format!("+ {}", describe_index(index)));
                add_indexes.push(AlterOp::AddIndex {
                    index: index_def(index),
                });
            }
            Some(other) if other.columns != index.columns || other.unique != index.unique => {
                details.push(format!(
                    "~ {} (was {})",
                    describe_index(index),
                    describe_index(other)
                ));
                drop_indexes.push(AlterOp::DropIndex {
                    name: index.name.clone(),
                });
                add_indexes.push(AlterOp::AddIndex {
                    index: index_def(index),
                });
            }
            Some(_) => {}
        }
    }
    for index in dst_indexes
        .iter()
        .filter(|i| !src_indexes.iter().any(|s| s.name == i.name))
    {
        details.push(format!("- {}", describe_index(index)));
        drop_indexes.push(AlterOp::DropIndex {
            name: index.name.clone(),
        });
    }

    // The old key goes before columns are dropped (one may be in it); the new one comes after
    // they are added.
    let mut drop_primary = None;
    let mut add_primary = None;
    if src.primary_key != dst.primary_key {
        details.push(
            match (dst.primary_key.is_empty(), src.primary_key.is_empty()) {
                (true, _) => format!("+ primary key ({})", src.primary_key.join(", ")),
                (_, true) => format!("- primary key ({})", dst.primary_key.join(", ")),
                _ => format!(
                    "~ primary key ({}) → ({})",
                    dst.primary_key.join(", "),
                    src.primary_key.join(", ")
                ),
            },
        );
        if !dst.primary_key.is_empty() {
            drop_primary = Some(AlterOp::DropPrimaryKey);
        }
        if !src.primary_key.is_empty() {
            add_primary = Some(AlterOp::AddPrimaryKey {
                columns: src.primary_key.clone(),
            });
        }
    }

    for fk in &src.foreign_keys {
        match dst.foreign_keys.iter().find(|o| o.name == fk.name) {
            None => details.push(format!("+ foreign key {}", describe_key(fk))),
            Some(other) if !same_key(fk, other) => {
                details.push(format!(
                    "~ foreign key {} (was {})",
                    describe_key(fk),
                    describe_key(other)
                ));
                found.drop_keys.push(AlterOp::DropForeignKey {
                    name: other.name.clone(),
                });
            }
            Some(_) => continue,
        }
        found.add_keys.push(AlterOp::AddForeignKey {
            foreign_key: fk.clone(),
        });
    }
    for other in dst
        .foreign_keys
        .iter()
        .filter(|o| !src.foreign_keys.iter().any(|f| f.name == o.name))
    {
        details.push(format!("- foreign key {}", describe_key(other)));
        found.drop_keys.push(AlterOp::DropForeignKey {
            name: other.name.clone(),
        });
    }

    let ops: Vec<AlterOp> = [
        drop_indexes,
        drop_primary.into_iter().collect(),
        add_columns,
        modify_columns,
        drop_columns,
        add_primary.into_iter().collect(),
        add_indexes,
    ]
    .concat();
    if !ops.is_empty() {
        found.middle.push(SchemaChange::AlterTable {
            table: table.clone(),
            ops,
        });
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, type_name: &str, nullable: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: type_name.into(),
            nullable,
            default: None,
            extra: None,
            comment: None,
            auto_increment: false,
            on_update: None,
            generated: false,
        }
    }

    fn index(name: &str, columns: &[&str], unique: bool, primary: bool) -> IndexInfo {
        IndexInfo {
            name: name.into(),
            columns: columns.iter().map(|c| (*c).to_owned()).collect(),
            unique,
            primary,
        }
    }

    fn key(name: &str, column: &str, table: &str, database: &str) -> ForeignKey {
        ForeignKey {
            name: name.into(),
            columns: vec![column.into()],
            references: TableRef {
                database: Some(database.into()),
                schema: None,
                name: table.into(),
            },
            referenced_columns: vec!["id".into()],
            on_update: None,
            on_delete: Some("CASCADE".into()),
        }
    }

    fn table(name: &str, columns: Vec<ColumnInfo>, indexes: Vec<IndexInfo>) -> TableInfo {
        TableInfo {
            table: TableRef {
                database: Some("src".into()),
                schema: None,
                name: name.into(),
            },
            columns,
            primary_key: vec!["id".into()],
            indexes,
            foreign_keys: Vec::new(),
            ddl: None,
            estimated_rows: None,
            view: false,
        }
    }

    fn orders() -> TableInfo {
        table(
            "orders",
            vec![
                column("id", "bigint", false),
                column("total", "decimal(12,2)", false),
                column("note", "varchar(255)", true),
            ],
            vec![
                index("PRIMARY", &["id"], true, true),
                index("orders_total", &["total"], false, false),
            ],
        )
    }

    fn diff(source: &[TableInfo], target: &[TableInfo]) -> Diff {
        diff_tables("src", "dst", source, target)
    }

    fn ops(change: &SchemaChange) -> Vec<&str> {
        let SchemaChange::AlterTable { ops, .. } = change else {
            return vec!["not an alter"];
        };
        ops.iter()
            .map(|op| match op {
                AlterOp::DropIndex { .. } => "drop index",
                AlterOp::AddColumn { .. } => "add column",
                AlterOp::ModifyColumn { .. } => "modify column",
                AlterOp::DropColumn { .. } => "drop column",
                AlterOp::AddIndex { .. } => "add index",
                AlterOp::AddPrimaryKey { .. } => "add primary key",
                AlterOp::DropPrimaryKey => "drop primary key",
                AlterOp::AddForeignKey { .. } => "add foreign key",
                AlterOp::DropForeignKey { .. } => "drop foreign key",
            })
            .collect()
    }

    #[test]
    fn same_tables_have_no_diff() {
        let found = diff(&[orders()], &[orders()]);
        assert!(found.tables.is_empty());
        assert!(found.changes.is_empty());
        assert_eq!(found.same, 1);
    }

    #[test]
    fn missing_tables_get_their_keys_after_every_table_exists() {
        let mut orders = orders();
        orders
            .foreign_keys
            .push(key("orders_customer", "customer_id", "customers", "src"));
        let customers = table("customers", vec![column("id", "bigint", false)], Vec::new());
        // "customers" sorts first, but the key would come before it if it were made with "orders".
        let found = diff(&[orders, customers], &[]);
        assert_eq!(found.tables.len(), 2);
        let kinds: Vec<&str> = found
            .changes
            .iter()
            .map(|c| match c {
                SchemaChange::CreateTable { table, .. } => table.name.as_str(),
                SchemaChange::AlterTable { .. } => "alter",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["customers", "orders", "alter", "alter"]);
        assert_eq!(ops(&found.changes[2]), ["add index"]);
        let SchemaChange::AlterTable { table, ops: last } = &found.changes[3] else {
            panic!("{:?}", found.changes)
        };
        assert_eq!(table.database.as_deref(), Some("dst"));
        let [AlterOp::AddForeignKey { foreign_key }] = last.as_slice() else {
            panic!("{last:?}")
        };
        // A key into the source's own database points into the target's.
        assert_eq!(foreign_key.references.database.as_deref(), Some("dst"));
    }

    #[test]
    fn extra_tables_lose_their_keys_before_any_is_dropped() {
        // "child" points at "parent"; dropping "parent" first would be refused.
        let mut child = table("child", vec![column("id", "bigint", false)], Vec::new());
        child
            .foreign_keys
            .push(key("child_parent", "id", "parent", "dst"));
        let parent = table("parent", vec![column("id", "bigint", false)], Vec::new());
        let found = diff(&[], &[parent, child]);
        let steps: Vec<String> = found
            .changes
            .iter()
            .map(|c| match c {
                SchemaChange::AlterTable { table, .. } => format!("alter {}", table.name),
                SchemaChange::DropTable { table, .. } => format!("drop {}", table.name),
                _ => "other".to_owned(),
            })
            .collect();
        assert_eq!(steps, ["alter child", "drop child", "drop parent"]);
        assert_eq!(ops(&found.changes[0]), ["drop foreign key"]);
    }

    #[test]
    fn extra_table_is_dropped() {
        let found = diff(&[], &[orders()]);
        assert_eq!(found.tables[0].kind, TableDiffKind::Extra);
        assert!(matches!(
            &found.changes[..],
            [SchemaChange::DropTable { view: false, .. }]
        ));
    }

    #[test]
    fn changed_columns_and_indexes_become_one_alter_in_a_safe_order() {
        let mut target = orders();
        target.columns[1] = column("total", "decimal(10,2)", true);
        target.columns.retain(|c| c.name != "note");
        target.columns.push(column("legacy", "int", true));
        target.indexes[1] = index("orders_total", &["total", "id"], false, false);
        target
            .indexes
            .push(index("orders_legacy", &["legacy"], false, false));

        let found = diff(&[orders()], &[target]);
        assert_eq!(found.tables[0].kind, TableDiffKind::Changed);
        let [change] = found.changes.as_slice() else {
            panic!("{:?}", found.changes)
        };
        assert_eq!(
            ops(change),
            [
                "drop index",
                "drop index",
                "add column",
                "modify column",
                "drop column",
                "add index"
            ]
        );
        assert!(
            found.tables[0].details.contains(
                &"~ column total: decimal(10,2) → decimal(12,2), NULL → NOT NULL".to_owned()
            ),
            "{:?}",
            found.tables[0].details
        );
    }

    #[test]
    fn type_and_action_case_is_not_a_difference() {
        let mut source = orders();
        source.foreign_keys.push(key("k", "id", "orders", "src"));
        let mut target = orders();
        target.columns[0].type_name = "BIGINT".into();
        let mut lower = key("k", "id", "orders", "dst");
        lower.on_delete = Some("cascade".into());
        target.foreign_keys.push(lower);
        let found = diff(&[source], &[target]);
        assert!(found.tables.is_empty(), "{:?}", found.tables);
        assert_eq!(found.same, 1);
    }

    #[test]
    fn keys_are_dropped_first_and_added_last() {
        let mut source = orders();
        source
            .foreign_keys
            .push(key("orders_customer", "customer_id", "customers", "src"));
        let mut target = orders();
        target.primary_key = vec!["id".into(), "total".into()];
        target
            .foreign_keys
            .push(key("orders_old", "total", "prices", "dst"));
        let mut changed = key("orders_customer", "customer_id", "customers", "dst");
        changed.on_delete = None;
        target.foreign_keys.push(changed);

        let found = diff(&[source], &[target]);
        let steps: Vec<Vec<&str>> = found.changes.iter().map(ops).collect();
        assert_eq!(
            steps,
            [
                vec!["drop foreign key", "drop foreign key"],
                vec!["drop primary key", "add primary key"],
                vec!["add foreign key"],
            ]
        );
        assert_eq!(
            found.tables[0].details,
            [
                "~ primary key (id, total) → (id)",
                "~ foreign key orders_customer (customer_id) → customers(id) (was orders_customer (customer_id) → customers(id))",
                "- foreign key orders_old (total) → prices(id)",
            ]
        );
    }
}
