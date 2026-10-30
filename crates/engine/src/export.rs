//! Exports: what a dialog needs to know before it opens, a preview, and writing rows the client
//! holds to a file. The formats themselves live in `quarry-export`, which never sees a database.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use quarry_driver_api::{Dialect, ExecItem, ExecOpts};
use quarry_export::{ExportError, Output, Summary, Writer, format_info};
use quarry_model::{
    BrowseQuery, ColumnMeta, ConnId, ExportFormat, ExportOptions, TableInfo, TableRef, Value,
};

use crate::engine::{Engine, describe};
use crate::event::ExportFormatState;
use crate::{Event, ExportSource, RequestId};

/// Rows a preview shows.
const PREVIEW_ROWS: usize = 20;

/// What a result on the grid can be exported as. A table export offers other formats: a result
/// is small and often wanted as statements or in a spreadsheet, a table is large and often wanted
/// as data for another tool.
pub(crate) const RESULT_FORMATS: [ExportFormat; 5] = [
    ExportFormat::SqlInserts,
    ExportFormat::SqlUpdates,
    ExportFormat::WhereClause,
    ExportFormat::Xlsx,
    ExportFormat::Csv,
];

pub(crate) const TABLE_FORMATS: [ExportFormat; 6] = [
    ExportFormat::Csv,
    ExportFormat::Tsv,
    ExportFormat::Json,
    ExportFormat::Ndjson,
    ExportFormat::SqlInserts,
    ExportFormat::Xlsx,
];

const NO_SINGLE_TABLE: &str = "The result mixes several tables or has computed columns, so there \
     is no table to find its rows in. Use SQL Inserts, CSV or Excel.";

impl Engine {
    /// Logs how long an export took (`phase` names it in the performance log) and tells the
    /// client the file is written.
    pub(crate) fn export_finished(
        &self,
        phase: &str,
        started: Instant,
        request: RequestId,
        path: String,
        format: ExportFormat,
        summary: &Summary,
    ) {
        self.perf(
            phase,
            started,
            serde_json::json!({ "rows": summary.rows, "format": format!("{format:?}") }),
        );
        self.emit(Event::ExportFinished {
            request,
            path,
            rows: summary.rows,
            sheets: summary.sheets,
            cut_cells: summary.cut_cells,
        });
    }

    pub(crate) async fn export_prepare(
        &self,
        request: RequestId,
        conn: ConnId,
        source: ExportSource,
    ) {
        let event = match source {
            ExportSource::Rows { columns } => self.prepare_result(request, conn, &columns).await,
            ExportSource::Table { table, query } => {
                self.prepare_table(request, conn, table, &query).await
            }
            ExportSource::Query { sql, columns } => {
                self.prepare_query(request, conn, &sql, &columns).await
            }
        };
        self.emit(event);
    }

    pub(crate) async fn export_preview(
        &self,
        request: RequestId,
        conn: ConnId,
        format: ExportFormat,
        options: ExportOptions,
        columns: Vec<ColumnMeta>,
        rows: Vec<Vec<Value>>,
    ) {
        let result = async {
            let (options, columns) = self.writer_input(conn, format, &options, &columns).await?;
            let driver = self.driver_of(conn)?;
            preview_text(driver.dialect(), format, &options, &columns, &rows)
        }
        .await;
        let (text, error) = match result {
            Ok(text) => (Some(text), None),
            Err(error) => (None, Some(error)),
        };
        self.emit(Event::ExportPreview {
            request,
            text,
            error,
        });
    }

    /// Write `rows` to `path`. The file is written on the blocking pool, so a slow disk never
    /// holds a worker that streams query results.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn export_rows(
        self: Arc<Self>,
        request: RequestId,
        conn: ConnId,
        format: ExportFormat,
        options: ExportOptions,
        columns: Vec<ColumnMeta>,
        rows: Vec<Vec<Value>>,
        path: String,
    ) {
        let started = Instant::now();
        let result = async {
            if path.trim().is_empty() {
                return Err("Choose a file to save to.".to_owned());
            }
            let (options, columns) = self.writer_input(conn, format, &options, &columns).await?;
            let engine = self.clone();
            let target = path.clone();
            tokio::task::spawn_blocking(move || {
                let driver = engine.driver_of(conn)?;
                write_rows(
                    driver.dialect(),
                    format,
                    &options,
                    &columns,
                    &rows,
                    Path::new(&target),
                )
                .map_err(|err| err.to_string())
            })
            .await
            .map_err(|_| "The export stopped unexpectedly. Try again.".to_owned())?
        }
        .await;
        match result {
            Ok(summary) => {
                self.export_finished("export.rows", started, request, path, format, &summary);
            }
            Err(error) => self.emit(Event::ExportFailed {
                request,
                error,
                cancelled: false,
            }),
        }
    }

    async fn prepare_result(
        &self,
        request: RequestId,
        conn: ConnId,
        columns: &[ColumnMeta],
    ) -> Event {
        let table = single_source(columns);
        let key = match &table {
            Some(table) => self.result_key(conn, table, columns).await,
            None => Err(NO_SINGLE_TABLE.to_owned()),
        };
        Event::ExportPrepared {
            request,
            formats: RESULT_FORMATS
                .iter()
                .map(|format| format_state(*format, &key))
                .collect(),
            table,
            columns: Vec::new(),
            rows: Vec::new(),
            warnings: Vec::new(),
            error: None,
        }
    }

    /// What the dialog of "export every row" needs: the same formats as the grid, plus what may
    /// make the second run differ from the first.
    async fn prepare_query(
        &self,
        request: RequestId,
        conn: ConnId,
        sql: &str,
        columns: &[ColumnMeta],
    ) -> Event {
        if let Err(error) = read_statement(sql) {
            return Event::ExportPrepared {
                request,
                formats: Vec::new(),
                table: None,
                columns: Vec::new(),
                rows: Vec::new(),
                warnings: Vec::new(),
                error: Some(error),
            };
        }
        let mut event = self.prepare_result(request, conn, columns).await;
        if let Event::ExportPrepared { warnings, .. } = &mut event {
            *warnings = session_warnings(sql);
        }
        event
    }

    async fn prepare_table(
        &self,
        request: RequestId,
        conn: ConnId,
        table: TableRef,
        query: &BrowseQuery,
    ) -> Event {
        match self.table_sample(conn, &table, query).await {
            Ok((columns, rows)) => Event::ExportPrepared {
                request,
                formats: TABLE_FORMATS
                    .iter()
                    .map(|format| format_state(*format, &Ok(Vec::new())))
                    .collect(),
                table: Some(table),
                columns,
                rows,
                warnings: Vec::new(),
                error: None,
            },
            Err(error) => Event::ExportPrepared {
                request,
                formats: Vec::new(),
                table: Some(table),
                columns: Vec::new(),
                rows: Vec::new(),
                warnings: Vec::new(),
                error: Some(error),
            },
        }
    }

    /// The first rows of a table with its filter and sort, for a preview. Read on the metadata
    /// session: a page of a few rows never holds it for long.
    async fn table_sample(
        &self,
        conn: ConnId,
        table: &TableRef,
        query: &BrowseQuery,
    ) -> Result<(Vec<ColumnMeta>, Vec<Vec<Value>>), String> {
        let page = self.page_query(conn, table, query, PREVIEW_ROWS as u64, 0)?;
        let meta = self.meta_session(conn).await?;
        self.with_meta(conn, meta, |session| {
            let sql = page.sql.clone();
            let params = page.params.clone();
            Box::pin(async move {
                let mut columns = Vec::new();
                let mut rows = Vec::new();
                let mut sink = |item: ExecItem| match item {
                    ExecItem::Columns(found) => columns = found,
                    ExecItem::Rows(mut batch) => rows.append(&mut batch),
                    ExecItem::Batch(batch) => rows.append(&mut batch.decode().unwrap_or_default()),
                    _ => {}
                };
                let opts = ExecOpts {
                    row_limit: Some(PREVIEW_ROWS as u64),
                    ..ExecOpts::default()
                };
                if params.is_empty() {
                    session.execute(&sql, &opts, &mut sink).await?;
                } else {
                    session.query(&sql, &params, &opts, &mut sink).await?;
                }
                Ok((columns, rows))
            })
        })
        .await
        .map_err(|err| describe(&err))
    }

    /// The key of `table` as it appears in the result, or why there is none. Read on the
    /// metadata session.
    async fn result_key(
        &self,
        conn: ConnId,
        table: &TableRef,
        columns: &[ColumnMeta],
    ) -> Result<Vec<String>, String> {
        let info = self
            .table_info_of(conn, table)
            .await
            .map_err(|err| format!("Could not read the keys of {}: {err}", table.name))?;
        choose_key(table, &key_candidates(&info), |name| {
            columns
                .iter()
                .any(|column| column.source_column.as_deref() == Some(name))
        })
    }

    /// What the writer gets for `format`: SQL names columns as the table does, not as the
    /// query aliased them, and the engine alone decides the table and key of statements that
    /// find a row (a client's own would let a statement hit the wrong rows).
    pub(crate) async fn writer_input(
        &self,
        conn: ConnId,
        format: ExportFormat,
        options: &ExportOptions,
        columns: &[ColumnMeta],
    ) -> Result<(ExportOptions, Vec<ColumnMeta>), String> {
        if columns.is_empty() {
            return Err("There is nothing to export.".to_owned());
        }
        let mut options = options.clone();
        let source = single_source(columns);
        let columns = if uses_table_names(format, &options, source.as_ref()) {
            sql_columns(columns)?
        } else {
            columns.to_vec()
        };
        match format {
            ExportFormat::SqlInserts if options.table.is_none() => options.table = source,
            ExportFormat::SqlUpdates | ExportFormat::WhereClause => {
                let table = source.ok_or(NO_SINGLE_TABLE)?;
                options.key = self.result_key(conn, &table, &columns).await?;
                options.table = Some(table);
            }
            _ => {}
        }
        Ok((options, columns))
    }
}

/// The one statement in `sql`, which must only read: "export every row" runs it again, and a
/// second run of a write would write twice.
pub(crate) fn read_statement(sql: &str) -> Result<&str, String> {
    let ranges = quarry_sql::split(sql);
    let [range] = ranges.as_slice() else {
        return Err(
            "Export every row works on one statement. Put the cursor on it, or select \
                    it."
            .to_owned(),
        );
    };
    let statement = &sql[range.clone()];
    if quarry_sql::classify(statement) != quarry_sql::StatementKind::Read {
        return Err(
            "Only a statement that reads, such as SELECT, can run again to export \
                    every row. Export the rows on the grid instead."
                .to_owned(),
        );
    }
    Ok(statement)
}

/// What a second run of `sql` may not find, in words for the user.
fn session_warnings(sql: &str) -> Vec<String> {
    let state = quarry_sql::session_state(sql);
    let mut warnings = Vec::new();
    if state.has_user_variables {
        warnings.push(
            "The query uses variables set with @name. They belong to the connection of the \
             tab, which the export does not use, so they will be empty."
                .to_owned(),
        );
    }
    if state.has_temporary_tables {
        warnings.push(
            "The query creates or drops a temporary table. Temporary tables belong to the \
             connection of the tab, so the export will not find them."
                .to_owned(),
        );
    }
    warnings
}

/// Statements use the column names of the table the rows come from, not the aliases the query
/// gave them. When the user names another table for SQL Inserts, the aliases are the names they
/// chose for it.
fn uses_table_names(
    format: ExportFormat,
    options: &ExportOptions,
    source: Option<&TableRef>,
) -> bool {
    match format {
        ExportFormat::SqlUpdates | ExportFormat::WhereClause => true,
        ExportFormat::SqlInserts => {
            source.is_some()
                && options
                    .table
                    .as_ref()
                    .is_none_or(|table| Some(table) == source)
        }
        _ => false,
    }
}

fn format_state(format: ExportFormat, key: &Result<Vec<String>, String>) -> ExportFormatState {
    let info = format_info(format);
    let reason = if info.is_key_required {
        key.as_ref().err().cloned()
    } else {
        None
    };
    ExportFormatState {
        format,
        extension: info.extension.to_owned(),
        has_preview: info.has_preview,
        max_rows_per_sheet: info.max_rows_per_sheet,
        is_available: reason.is_none(),
        reason,
    }
}

/// The one table every column of a result comes from. A join, or a computed column, has none.
fn single_source(columns: &[ColumnMeta]) -> Option<TableRef> {
    let first = columns.first()?;
    let name = first.source_table.as_ref()?;
    columns
        .iter()
        .all(|column| {
            column.source_table.as_ref() == Some(name)
                && column.source_database == first.source_database
        })
        .then(|| TableRef {
            database: first.source_database.clone(),
            schema: None,
            name: name.clone(),
        })
}

/// The keys a table offers to find one row, best first: the primary key, then each unique index
/// whose columns are all NOT NULL (a NULL never equals itself, so it would match no row).
fn key_candidates(info: &TableInfo) -> Vec<Vec<String>> {
    if info.view {
        return Vec::new();
    }
    let mut keys = Vec::new();
    if !info.primary_key.is_empty() {
        keys.push(info.primary_key.clone());
    }
    for index in &info.indexes {
        let is_not_null = index.columns.iter().all(|name| {
            info.columns
                .iter()
                .any(|column| &column.name == name && !column.nullable)
        });
        if index.unique
            && !index.primary
            && !index.columns.is_empty()
            && is_not_null
            && !keys.contains(&index.columns)
        {
            keys.push(index.columns.clone());
        }
    }
    keys
}

/// The first key whose columns the result holds, or what the user can do about it.
fn choose_key(
    table: &TableRef,
    candidates: &[Vec<String>],
    has_column: impl Fn(&str) -> bool,
) -> Result<Vec<String>, String> {
    let Some(best) = candidates.first() else {
        return Err(format!(
            "{} has no primary key or unique key on NOT NULL columns, so its rows cannot be \
             found again. Use SQL Inserts, CSV or Excel.",
            table.name
        ));
    };
    candidates
        .iter()
        .find(|key| key.iter().all(|name| has_column(name)))
        .cloned()
        .ok_or_else(|| {
            format!(
                "The result lacks the key of {}: {}. Add it to the SELECT.",
                table.name,
                best.join(", ")
            )
        })
}

/// `columns` named as the table names them. Two result columns of the same table column would
/// give a statement the same column twice.
fn sql_columns(columns: &[ColumnMeta]) -> Result<Vec<ColumnMeta>, String> {
    let named: Vec<ColumnMeta> = columns
        .iter()
        .map(|column| ColumnMeta {
            name: column
                .source_column
                .clone()
                .unwrap_or_else(|| column.name.clone()),
            ..column.clone()
        })
        .collect();
    for (index, column) in named.iter().enumerate() {
        if named[..index]
            .iter()
            .any(|earlier| earlier.name == column.name)
        {
            return Err(format!(
                "The column {} appears twice in the result. Remove one, or use CSV or Excel.",
                column.name
            ));
        }
    }
    Ok(named)
}

/// The first rows as `format` writes them. The same writer as the file, so they cannot differ.
fn preview_text(
    dialect: &dyn Dialect,
    format: ExportFormat,
    options: &ExportOptions,
    columns: &[ColumnMeta],
    rows: &[Vec<Value>],
) -> Result<String, String> {
    if !format_info(format).has_preview {
        return Err("This format has no preview. Export it to see the result.".to_owned());
    }
    let shown = &rows[..rows.len().min(PREVIEW_ROWS)];
    let render = || -> Result<Vec<u8>, ExportError> {
        let mut writer = Writer::start(format, options, dialect, Vec::new(), columns)?;
        writer.write_batch(shown)?;
        writer.finish()
    };
    let bytes = render().map_err(|err| err.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}

/// Write `rows` to `path`. Any failure leaves no file: the output is removed unless it was
/// committed, and a file that was there before stays as it was.
fn write_rows(
    dialect: &dyn Dialect,
    format: ExportFormat,
    options: &ExportOptions,
    columns: &[ColumnMeta],
    rows: &[Vec<Value>],
    path: &Path,
) -> Result<Summary, ExportError> {
    let output = Output::create(path)?;
    let mut writer = Writer::start(format, options, dialect, output, columns)?;
    writer.write_batch(rows)?;
    let summary = writer.summary();
    writer.finish()?.commit()?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use quarry_model::{ColumnInfo, IndexInfo};

    use super::*;
    use crate::edit::tests::Ansi;

    fn column(name: &str, table: Option<&str>, source: Option<&str>) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            type_name: "TEXT".into(),
            nullable: None,
            source_database: table.map(|_| "shop".to_owned()),
            source_table: table.map(str::to_owned),
            source_column: source.map(str::to_owned),
        }
    }

    fn orders() -> TableRef {
        TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "orders".into(),
        }
    }

    fn info(columns: &[(&str, bool)], pk: &[&str], indexes: Vec<IndexInfo>) -> TableInfo {
        TableInfo {
            table: orders(),
            columns: columns
                .iter()
                .map(|(name, nullable)| ColumnInfo {
                    name: (*name).into(),
                    type_name: "int".into(),
                    nullable: *nullable,
                    default: None,
                    extra: None,
                    comment: None,
                    auto_increment: false,
                    on_update: None,
                    generated: false,
                })
                .collect(),
            primary_key: pk.iter().map(|c| (*c).to_owned()).collect(),
            indexes,
            foreign_keys: Vec::new(),
            ddl: None,
            estimated_rows: None,
            view: false,
        }
    }

    fn unique(name: &str, columns: &[&str]) -> IndexInfo {
        IndexInfo {
            name: name.into(),
            columns: columns.iter().map(|c| (*c).to_owned()).collect(),
            unique: true,
            primary: false,
        }
    }

    #[test]
    fn a_result_has_one_source_table_only_when_every_column_shares_it() {
        let same = [
            column("id", Some("orders"), Some("id")),
            column("total", Some("orders"), Some("total")),
        ];
        assert_eq!(single_source(&same), Some(orders()));
        let join = [
            column("id", Some("orders"), Some("id")),
            column("name", Some("customers"), Some("name")),
        ];
        assert_eq!(single_source(&join), None);
        let computed = [
            column("id", Some("orders"), Some("id")),
            column("n", None, None),
        ];
        assert_eq!(single_source(&computed), None);
        let mut other_db = [
            column("id", Some("orders"), Some("id")),
            column("total", Some("orders"), Some("total")),
        ];
        other_db[1].source_database = Some("archive".into());
        assert_eq!(single_source(&other_db), None);
        assert_eq!(single_source(&[]), None);
    }

    #[test]
    fn keys_are_the_primary_key_then_unique_indexes_on_not_null_columns() {
        let table = info(
            &[
                ("id", false),
                ("code", false),
                ("email", true),
                ("sku", false),
            ],
            &["id"],
            vec![
                unique("code", &["code"]),
                unique("email", &["email"]),
                unique("pk_again", &["id"]),
                unique("expr", &["lower(sku)"]),
            ],
        );
        assert_eq!(
            key_candidates(&table),
            vec![vec!["id".to_owned()], vec!["code".to_owned()]],
            "a nullable unique key, an expression and a copy of the primary key are not keys"
        );
        let no_pk = info(&[("code", false)], &[], vec![unique("code", &["code"])]);
        assert_eq!(key_candidates(&no_pk), vec![vec!["code".to_owned()]]);
        let mut view = info(&[("id", false)], &["id"], Vec::new());
        view.view = true;
        assert!(key_candidates(&view).is_empty());
    }

    #[test]
    fn a_key_is_chosen_from_what_the_result_holds() {
        let candidates = vec![vec!["id".to_owned()], vec!["code".to_owned()]];
        let table = orders();
        assert_eq!(
            choose_key(&table, &candidates, |c| c == "id" || c == "code"),
            Ok(vec!["id".to_owned()])
        );
        assert_eq!(
            choose_key(&table, &candidates, |c| c == "code"),
            Ok(vec!["code".to_owned()]),
            "a unique key stands in when the primary key was not selected"
        );
        let missing = choose_key(&table, &candidates, |_| false).unwrap_err();
        assert!(
            missing.contains("orders") && missing.contains("id"),
            "{missing}"
        );
        assert!(missing.contains("Add it to the SELECT"));
        let none = choose_key(&table, &[], |_| true).unwrap_err();
        assert!(none.contains("no primary key"), "{none}");
    }

    #[test]
    fn sql_names_columns_as_the_table_does_not_as_the_query_aliased_them() {
        let columns = [
            column("order_id", Some("orders"), Some("id")),
            column("total", Some("orders"), Some("total")),
            column("n", None, None),
        ];
        let named: Vec<String> = sql_columns(&columns)
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(
            named,
            ["id", "total", "n"],
            "a computed column keeps its alias"
        );
        let twice = [
            column("id", Some("orders"), Some("id")),
            column("again", Some("orders"), Some("id")),
        ];
        let err = sql_columns(&twice).unwrap_err();
        assert!(err.contains("id") && err.contains("twice"), "{err}");
    }

    #[test]
    fn aliases_become_table_names_unless_the_user_names_another_table() {
        let orders = orders();
        let other = TableRef {
            name: "orders_archive".into(),
            ..orders.clone()
        };
        let with = |table: Option<&TableRef>| ExportOptions {
            table: table.cloned(),
            ..ExportOptions::default()
        };
        let uses = |format, table, source| uses_table_names(format, &with(table), source);
        let inserts = ExportFormat::SqlInserts;
        assert!(uses(inserts, None, Some(&orders)));
        assert!(uses(inserts, Some(&orders), Some(&orders)));
        assert!(!uses(inserts, Some(&other), Some(&orders)));
        assert!(
            !uses(inserts, Some(&other), None),
            "a join: the aliases are the names"
        );
        assert!(!uses(inserts, None, None));
        assert!(uses(ExportFormat::SqlUpdates, None, Some(&orders)));
        assert!(uses(ExportFormat::WhereClause, Some(&other), Some(&orders)));
        assert!(
            !uses(ExportFormat::Csv, None, Some(&orders)),
            "CSV keeps the aliases"
        );
    }

    #[test]
    fn a_format_that_needs_a_key_says_why_it_cannot_be_used() {
        let no_key: Result<Vec<String>, String> = Err("no key".into());
        let updates = format_state(ExportFormat::SqlUpdates, &no_key);
        assert!(!updates.is_available);
        assert_eq!(updates.reason.as_deref(), Some("no key"));
        let csv = format_state(ExportFormat::Csv, &no_key);
        assert!(csv.is_available && csv.reason.is_none());
        assert!(format_state(ExportFormat::SqlUpdates, &Ok(vec!["id".into()])).is_available);
        let excel = format_state(ExportFormat::Xlsx, &Ok(Vec::new()));
        assert!(!excel.has_preview);
        assert_eq!(excel.max_rows_per_sheet, Some(1_048_576));
    }

    #[test]
    fn a_preview_shows_the_first_rows_only_and_hides_the_byte_order_mark() {
        let columns = [column("id", Some("orders"), Some("id"))];
        let rows: Vec<Vec<Value>> = (0..100).map(|n| vec![Value::Int(n)]).collect();
        let options = ExportOptions {
            has_bom: true,
            ..ExportOptions::default()
        };
        let text = preview_text(&Ansi, ExportFormat::Csv, &options, &columns, &rows).unwrap();
        assert_eq!(text.lines().count(), 1 + PREVIEW_ROWS);
        assert!(text.starts_with("id\r\n0\r\n"), "{text:?}");
    }

    #[test]
    fn writing_leaves_the_whole_file_or_nothing() {
        let dir = std::env::temp_dir().join(format!("quarry-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let columns = [column("id", None, None)];
        let rows = vec![vec![Value::Int(1)], vec![Value::Int(2)]];
        let target = dir.join("out.csv");
        let summary = write_rows(
            &Ansi,
            ExportFormat::Csv,
            &ExportOptions::default(),
            &columns,
            &rows,
            &target,
        )
        .unwrap();
        assert_eq!(summary.rows, 2);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "id\r\n1\r\n2\r\n"
        );

        // SQL Inserts without a table fails before it writes, and leaves the first file alone.
        let err = write_rows(
            &Ansi,
            ExportFormat::SqlInserts,
            &ExportOptions::default(),
            &columns,
            &rows,
            &target,
        )
        .unwrap_err();
        assert!(matches!(err, ExportError::TableRequired));
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "id\r\n1\r\n2\r\n"
        );
        assert!(!dir.join("out.csv.part").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
