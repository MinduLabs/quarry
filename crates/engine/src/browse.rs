//! Turns a [`BrowseQuery`] into `WHERE` / `ORDER BY` clauses. Column names are quoted by the
//! dialect and values are parameters, so neither can change the statement.

use quarry_driver_api::Dialect;
use quarry_model::{BrowseQuery, ColumnFilter, FilterOp, SortKey, Value};

/// Clauses without their keywords, ready for `Dialect::select_page` / `count_rows`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Clauses {
    pub filter: Option<String>,
    pub order: Option<String>,
    pub params: Vec<Value>,
}

/// A sorted query with the primary key appended to its sort. Rows that tie on the sort columns
/// otherwise come back in a different order for each `LIMIT` / `OFFSET`, so pages repeat or skip
/// rows. An unsorted query keeps the table's own order.
pub fn with_tie_breakers(mut query: BrowseQuery, primary_key: &[String]) -> BrowseQuery {
    if query.sort.is_empty() {
        return query;
    }
    for column in primary_key {
        if !query
            .sort
            .iter()
            .any(|k| k.column.eq_ignore_ascii_case(column))
        {
            query.sort.push(SortKey {
                column: column.clone(),
                descending: false,
            });
        }
    }
    query
}

pub fn clauses(dialect: &dyn Dialect, query: &BrowseQuery) -> Result<Clauses, String> {
    let mut params = Vec::new();
    let conditions = query
        .filters
        .iter()
        .map(|f| condition(dialect, f, &mut params))
        .collect::<Result<Vec<_>, _>>()?;
    let keys = query
        .sort
        .iter()
        .map(|key| {
            let column = column(dialect, &key.column)?;
            Ok(format!(
                "{column} {}",
                if key.descending { "DESC" } else { "ASC" }
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Clauses {
        filter: (!conditions.is_empty()).then(|| conditions.join(" AND ")),
        order: (!keys.is_empty()).then(|| keys.join(", ")),
        params,
    })
}

fn condition(
    dialect: &dyn Dialect,
    filter: &ColumnFilter,
    params: &mut Vec<Value>,
) -> Result<String, String> {
    let column = column(dialect, &filter.column)?;
    let mut param = |value: String| {
        params.push(Value::Text(value));
        dialect.placeholder(params.len())
    };
    let value = filter.value.clone();
    Ok(match filter.op {
        FilterOp::Eq => format!("{column} = {}", param(value)),
        FilterOp::NotEq => format!("{column} <> {}", param(value)),
        FilterOp::Lt => format!("{column} < {}", param(value)),
        FilterOp::Lte => format!("{column} <= {}", param(value)),
        FilterOp::Gt => format!("{column} > {}", param(value)),
        FilterOp::Gte => format!("{column} >= {}", param(value)),
        FilterOp::Contains => format!("{column} LIKE {}", param(format!("%{}%", like(&value)))),
        FilterOp::NotContains => {
            format!("{column} NOT LIKE {}", param(format!("%{}%", like(&value))))
        }
        FilterOp::StartsWith => format!("{column} LIKE {}", param(format!("{}%", like(&value)))),
        FilterOp::In => {
            let items: Vec<String> = value
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| param(v.to_owned()))
                .collect();
            if items.is_empty() {
                return Err(format!(
                    "The filter on “{}” needs at least one value, separated by commas.",
                    filter.column
                ));
            }
            format!("{column} IN ({})", items.join(", "))
        }
        FilterOp::IsNull => format!("{column} IS NULL"),
        FilterOp::IsNotNull => format!("{column} IS NOT NULL"),
    })
}

fn column(dialect: &dyn Dialect, name: &str) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("Pick a column for each filter and sort.".to_owned());
    }
    Ok(dialect.quote_ident(name))
}

/// Escape `LIKE` wildcards so the text matches literally. `\` is the default escape character.
fn like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use quarry_model::SortKey;

    use super::*;
    use crate::edit::tests::Ansi;

    fn filter(column: &str, op: FilterOp, value: &str) -> ColumnFilter {
        ColumnFilter {
            column: column.into(),
            op,
            value: value.into(),
        }
    }

    fn text(s: &str) -> Value {
        Value::Text(s.into())
    }

    #[test]
    fn sorted_queries_get_the_primary_key_as_tie_breaker() {
        let key = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        let sorted = |column: &str| BrowseQuery {
            filters: Vec::new(),
            sort: vec![SortKey {
                column: column.into(),
                descending: true,
            }],
        };
        let order = |query: &BrowseQuery| {
            query
                .sort
                .iter()
                .map(|k| (k.column.clone(), k.descending))
                .collect::<Vec<_>>()
        };

        let tied = with_tie_breakers(sorted("status"), &key(&["id", "shard"]));
        assert_eq!(
            order(&tied),
            [
                ("status".to_owned(), true),
                ("id".to_owned(), false),
                ("shard".to_owned(), false)
            ]
        );
        // A key column the user already sorts by keeps the user's direction and place.
        let keyed = with_tie_breakers(sorted("ID"), &key(&["id", "shard"]));
        assert_eq!(
            order(&keyed),
            [("ID".to_owned(), true), ("shard".to_owned(), false)]
        );
        // Unsorted pages and tables without a key stay as they are.
        assert_eq!(
            with_tie_breakers(BrowseQuery::default(), &key(&["id"])),
            BrowseQuery::default()
        );
        assert_eq!(with_tie_breakers(sorted("a"), &[]), sorted("a"));
    }

    #[test]
    fn empty_query_has_no_clauses() {
        assert_eq!(
            clauses(&Ansi, &BrowseQuery::default()).unwrap(),
            Clauses::default()
        );
    }

    #[test]
    fn filters_become_parameters() {
        let query = BrowseQuery {
            filters: vec![
                filter("status", FilterOp::Eq, "paid"),
                filter("total", FilterOp::Gte, "10"),
                filter("note", FilterOp::Contains, "50%_off\\"),
                filter("deleted_at", FilterOp::IsNull, "ignored"),
                filter("id", FilterOp::In, " 1, 2 ,,3 "),
            ],
            sort: vec![
                SortKey {
                    column: "created".into(),
                    descending: true,
                },
                SortKey {
                    column: "id".into(),
                    descending: false,
                },
            ],
        };
        let c = clauses(&Ansi, &query).unwrap();
        assert_eq!(
            c.filter.as_deref(),
            Some(
                "\"status\" = $1 AND \"total\" >= $2 AND \"note\" LIKE $3 \
                 AND \"deleted_at\" IS NULL AND \"id\" IN ($4, $5, $6)"
            )
        );
        assert_eq!(c.order.as_deref(), Some("\"created\" DESC, \"id\" ASC"));
        assert_eq!(
            c.params,
            vec![
                text("paid"),
                text("10"),
                text("%50\\%\\_off\\\\%"),
                text("1"),
                text("2"),
                text("3")
            ]
        );
    }

    #[test]
    fn hostile_names_and_values_stay_inert() {
        let query = BrowseQuery {
            filters: vec![filter("a\" = 1 OR \"b", FilterOp::Eq, "x' OR '1'='1")],
            sort: vec![],
        };
        let c = clauses(&Ansi, &query).unwrap();
        assert_eq!(c.filter.as_deref(), Some("\"a\"\" = 1 OR \"\"b\" = $1"));
        assert_eq!(c.params, vec![text("x' OR '1'='1")]);
    }

    #[test]
    fn rejects_incomplete_filters() {
        let empty_in = BrowseQuery {
            filters: vec![filter("id", FilterOp::In, " , ")],
            sort: vec![],
        };
        assert!(clauses(&Ansi, &empty_in).is_err());
        let no_column = BrowseQuery {
            filters: vec![filter(" ", FilterOp::Eq, "1")],
            sort: vec![],
        };
        assert!(clauses(&Ansi, &no_column).is_err());
    }
}
