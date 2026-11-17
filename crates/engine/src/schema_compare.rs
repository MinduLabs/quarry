//! Schema compare: what to change in a target database so its tables match a source database,
//! as `SchemaChange`s the target's driver turns into a script. Applying it is reviewed like any
//! schema change.

use std::collections::BTreeMap;
use std::time::Instant;

use quarry_model::{
    AlterOp, ColumnDef, ColumnInfo, ConnId, IndexDef, IndexInfo, ObjectKind, SchemaChange,
    SchemaDiff, SchemaSide, TableDiff, TableDiffKind, TableInfo, TableRef,
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
        let (tables, same) = diff_tables(&target.database, &source_tables, &target_tables);
        let driver = self.driver_of(target.conn)?;
        let mut statements = Vec::new();
        for change in tables.iter().flat_map(|t| &t.changes) {
            statements.extend(schema::checked(driver.schema_change(change).await)?);
        }
        Ok(SchemaDiff {
            source,
            target,
            tables,
            same,
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

/// The tables that differ between `source` and `target` (by name), and how many are the same.
/// Changes are for `target_db`.
pub(crate) fn diff_tables(
    target_db: &str,
    source: &[TableInfo],
    target: &[TableInfo],
) -> (Vec<TableDiff>, u32) {
    let by_name = |tables: &[TableInfo]| -> BTreeMap<String, TableInfo> {
        tables
            .iter()
            .map(|t| (t.table.name.clone(), t.clone()))
            .collect()
    };
    let source = by_name(source);
    let target = by_name(target);
    let table_ref = |name: &str| TableRef {
        database: Some(target_db.to_owned()),
        schema: None,
        name: name.to_owned(),
    };
    let mut diffs = Vec::new();
    let mut same = 0;
    for (name, src) in &source {
        match target.get(name) {
            None => diffs.push(missing_table(table_ref(name), src)),
            Some(dst) => match changed_table(table_ref(name), src, dst) {
                Some(diff) => diffs.push(diff),
                None => same += 1,
            },
        }
    }
    for name in target.keys().filter(|name| !source.contains_key(*name)) {
        diffs.push(TableDiff {
            name: name.clone(),
            kind: TableDiffKind::Extra,
            details: vec![format!("- table {name}")],
            changes: vec![SchemaChange::DropTable {
                table: table_ref(name),
                view: false,
            }],
        });
    }
    diffs.sort_by(|a, b| a.name.cmp(&b.name));
    (diffs, same)
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

fn missing_table(table: TableRef, src: &TableInfo) -> TableDiff {
    let name = table.name.clone();
    let mut changes = vec![SchemaChange::CreateTable {
        table: table.clone(),
        columns: src.columns.iter().map(column_def).collect(),
        primary_key: src.primary_key.clone(),
    }];
    let indexes: Vec<AlterOp> = src
        .indexes
        .iter()
        .filter(|i| !i.primary)
        .map(|i| AlterOp::AddIndex {
            index: index_def(i),
        })
        .collect();
    if !indexes.is_empty() {
        changes.push(SchemaChange::AlterTable {
            table,
            ops: indexes,
        });
    }
    let mut details = vec![format!("+ table {name} ({} columns)", src.columns.len())];
    details.extend(foreign_key_notes(src, None));
    TableDiff {
        name,
        kind: TableDiffKind::Missing,
        details,
        changes,
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

fn changed_table(table: TableRef, src: &TableInfo, dst: &TableInfo) -> Option<TableDiff> {
    let mut details = Vec::new();
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

    if src.primary_key != dst.primary_key {
        details.push(format!(
            "~ primary key ({}) → ({}): not scripted",
            dst.primary_key.join(", "),
            src.primary_key.join(", ")
        ));
    }
    details.extend(foreign_key_notes(src, Some(dst)));

    if details.is_empty() {
        return None;
    }
    // Indexes go first so a dropped column is no longer in one; new indexes come last, after
    // the columns they use exist.
    let ops: Vec<AlterOp> = [
        drop_indexes,
        add_columns,
        modify_columns,
        drop_columns,
        add_indexes,
    ]
    .into_iter()
    .flatten()
    .collect();
    let changes = if ops.is_empty() {
        Vec::new()
    } else {
        vec![SchemaChange::AlterTable {
            table: table.clone(),
            ops,
        }]
    };
    Some(TableDiff {
        name: table.name,
        kind: TableDiffKind::Changed,
        details,
        changes,
    })
}

/// Foreign keys that differ. Quarry has no change for them yet, so they are only reported.
fn foreign_key_notes(src: &TableInfo, dst: Option<&TableInfo>) -> Vec<String> {
    let describe = |fk: &quarry_model::ForeignKey| {
        format!(
            "{} ({}) → {}({})",
            fk.name,
            fk.columns.join(", "),
            fk.references.name,
            fk.referenced_columns.join(", ")
        )
    };
    let empty = Vec::new();
    let dst_keys = dst.map_or(&empty, |d| &d.foreign_keys);
    let mut notes = Vec::new();
    for fk in &src.foreign_keys {
        let other = dst_keys.iter().find(|o| o.name == fk.name);
        let differs = other.is_none_or(|o| {
            o.columns != fk.columns
                || o.references.name != fk.references.name
                || o.referenced_columns != fk.referenced_columns
        });
        if differs {
            notes.push(format!("+ foreign key {}: not scripted", describe(fk)));
        }
    }
    for fk in dst_keys
        .iter()
        .filter(|o| !src.foreign_keys.iter().any(|f| f.name == o.name))
    {
        notes.push(format!("- foreign key {}: not scripted", describe(fk)));
    }
    notes
}

#[cfg(test)]
mod tests {
    use quarry_model::ForeignKey;

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

    #[test]
    fn same_tables_have_no_diff() {
        let (diffs, same) = diff_tables("dst", &[orders()], &[orders()]);
        assert!(diffs.is_empty());
        assert_eq!(same, 1);
    }

    #[test]
    fn missing_table_is_created_with_its_indexes() {
        let (diffs, same) = diff_tables("dst", &[orders()], &[]);
        assert_eq!(same, 0);
        let [diff] = diffs.as_slice() else {
            panic!("{diffs:?}")
        };
        assert_eq!(diff.kind, TableDiffKind::Missing);
        let [
            SchemaChange::CreateTable {
                table,
                columns,
                primary_key,
            },
            SchemaChange::AlterTable { ops, .. },
        ] = diff.changes.as_slice()
        else {
            panic!("{:?}", diff.changes)
        };
        assert_eq!(table.database.as_deref(), Some("dst"));
        assert_eq!(columns.len(), 3);
        assert_eq!(primary_key, &["id".to_owned()]);
        assert!(matches!(&ops[..], [AlterOp::AddIndex { index }] if index.name == "orders_total"));
    }

    #[test]
    fn extra_table_is_dropped() {
        let (diffs, _) = diff_tables("dst", &[], &[orders()]);
        let [diff] = diffs.as_slice() else {
            panic!("{diffs:?}")
        };
        assert_eq!(diff.kind, TableDiffKind::Extra);
        assert!(matches!(
            &diff.changes[..],
            [SchemaChange::DropTable { view: false, .. }]
        ));
    }

    #[test]
    fn changed_columns_and_indexes_become_one_alter_in_a_safe_order() {
        let mut target = orders();
        // Older type, an extra column, a missing one, and an index on other columns.
        target.columns[1] = column("total", "decimal(10,2)", true);
        target.columns.retain(|c| c.name != "note");
        target.columns.push(column("legacy", "int", true));
        target.indexes[1] = index("orders_total", &["total", "id"], false, false);
        target
            .indexes
            .push(index("orders_legacy", &["legacy"], false, false));

        let (diffs, _) = diff_tables("dst", &[orders()], &[target]);
        let [diff] = diffs.as_slice() else {
            panic!("{diffs:?}")
        };
        assert_eq!(diff.kind, TableDiffKind::Changed);
        let [SchemaChange::AlterTable { ops, .. }] = diff.changes.as_slice() else {
            panic!("{:?}", diff.changes)
        };
        let kinds: Vec<&str> = ops
            .iter()
            .map(|op| match op {
                AlterOp::DropIndex { .. } => "drop index",
                AlterOp::AddColumn { .. } => "add column",
                AlterOp::ModifyColumn { .. } => "modify column",
                AlterOp::DropColumn { .. } => "drop column",
                AlterOp::AddIndex { .. } => "add index",
            })
            .collect();
        assert_eq!(
            kinds,
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
            diff.details.contains(
                &"~ column total: decimal(10,2) → decimal(12,2), NULL → NOT NULL".to_owned()
            ),
            "{:?}",
            diff.details
        );
    }

    #[test]
    fn type_case_is_not_a_difference() {
        let mut target = orders();
        target.columns[0].type_name = "BIGINT".into();
        let (diffs, same) = diff_tables("dst", &[orders()], &[target]);
        assert!(diffs.is_empty(), "{diffs:?}");
        assert_eq!(same, 1);
    }

    #[test]
    fn keys_are_reported_but_not_scripted() {
        let mut source = orders();
        source.foreign_keys.push(ForeignKey {
            name: "orders_customer".into(),
            columns: vec!["customer_id".into()],
            references: TableRef {
                database: Some("src".into()),
                schema: None,
                name: "customers".into(),
            },
            referenced_columns: vec!["id".into()],
            on_update: None,
            on_delete: None,
        });
        let mut target = orders();
        target.primary_key = vec!["id".into(), "total".into()];
        let (diffs, _) = diff_tables("dst", &[source], &[target]);
        let [diff] = diffs.as_slice() else {
            panic!("{diffs:?}")
        };
        assert!(diff.changes.is_empty());
        assert_eq!(
            diff.details,
            [
                "~ primary key (id, total) → (id): not scripted",
                "+ foreign key orders_customer (customer_id) → customers(id): not scripted",
            ]
        );
    }
}
