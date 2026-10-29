---
date: 2026-10-29
topic: data-export
---

# Data Export

## Summary

Quarry gets two ways to write data to a file, sharing one set of format writers. A result export saves the query result already on the grid, with a preview. A table export, opened by right-clicking a table, streams a whole table or a filtered view in any size. Both show the row count and let the user choose how many rows to write. Import is not part of this work.

---

## Problem Frame

Quarry users are data analysts, backend developers and data engineers. They work with databases all day and need data to leave the app: a spreadsheet for a report, a SQL script to seed another database, a columnar file for a pipeline.

Today the only way out is copying a selected block of cells to the clipboard as TSV, CSV, JSON or INSERT statements. That cannot carry a large table, cannot write a file, and cannot produce Excel or Parquet. The grid itself holds only a bounded number of rows per result (500 at the time of writing), so even a query that returns a million rows can be copied only in part. Users with a big table or a big join have no path at all.

Comparable tools (DataGrip, DBeaver, TablePlus) put export in two places: a format menu on the result grid, and an Import/Export submenu on a table in the tree. Users coming from them expect both.

---

## Actors

- A1. Data analyst: exports a result or a table to CSV or Excel to share or chart it.
- A2. Backend developer: exports rows as SQL INSERT, UPDATE or a WHERE clause to seed, patch or filter another database.
- A3. Data engineer: exports large tables or join results to Parquet, NDJSON or CSV for a pipeline.

---

## Key Flows

- F1. Export the result on the grid
  - **Trigger:** A query has finished and the user chooses export on the result.
  - **Actors:** A1, A2
  - **Steps:** The user picks a format. The dialog shows how many rows the grid holds and lets the user choose how many to write. For text formats, a preview shows the first rows as they will appear in the file. The user picks a file and confirms.
  - **Outcome:** A complete file exists with the chosen rows. No query is run again.
  - **Covered by:** R1, R2, R3, R4, R5, R6

- F2. Export the full result of a truncated query
  - **Trigger:** The result on the grid was cut at the row limit and the user wants every row.
  - **Actors:** A1, A3
  - **Steps:** The dialog offers "Export all" next to the grid-only choice. It says the query will run again on its own connection and lists the risks. The user confirms. Progress shows rows written so far. The user can cancel at any time.
  - **Outcome:** The file holds every row the query returns, or no file exists if the export was cancelled or failed.
  - **Covered by:** R7, R8, R9, R10, R15, R16

- F3. Export a whole table
  - **Trigger:** The user right-clicks a table in the tree and opens Import/Export, then the export entry.
  - **Actors:** A1, A2, A3
  - **Steps:** The dialog shows the table's total row count (after the filter currently applied to the table, if any). The user picks a format from the wider set, chooses how many rows to write, and picks a file. Progress shows rows and size written. The user can cancel.
  - **Outcome:** The file holds a consistent copy of the chosen rows.
  - **Covered by:** R11, R12, R13, R14, R15, R16, R17

---

## Requirements

**Result export (from the grid)**
- R1. A result on the grid can be exported to a file after the query has finished, from the result's own toolbar.
- R2. Formats for a result export: SQL Inserts, SQL Updates, Where Clause, Excel (xlsx), CSV.
- R3. By default a result export writes the rows already on the grid and runs no query.
- R4. The dialog shows how many rows the grid holds and lets the user choose how many to write (the first N). When the query returned more rows than the grid holds, the dialog says so.
- R5. SQL Updates and Where Clause are available only when the result comes from exactly one table with a usable key. Otherwise they are disabled and the dialog says why.
- R6. Every text format (SQL Inserts, SQL Updates, Where Clause, CSV) shows a preview of the first rows before the file is written. Excel has no preview.

**Export all rows of a query**
- R7. When the grid result was cut at the row limit, the dialog offers "Export all". It runs the same SQL again on a separate connection and writes every row.
- R8. "Export all" is allowed only for statements classified as reads. Other statements are refused with a reason.
- R9. Before "Export all" starts, the dialog states that the query runs again, that session variables and temporary tables from the tab are not available to it, and that the data may differ from the grid. When the statement text suggests session state is in use, the warning says so.
- R10. For SQL Inserts from a join (no single source table), the user names the target table.

**Table export**
- R11. Right-clicking a table in the tree shows an Import/Export entry with an export action. The import entries are not part of this work.
- R12. A table export offers a wider set of formats: CSV, TSV, JSON, NDJSON, SQL Inserts, Excel (xlsx), Parquet.
- R13. The dialog shows the table's total row count and lets the user choose how many rows to write. When a filter is applied to the table, the count and the export follow it.
- R14. A table export reads the rows as they were at one point in time, so the file never mixes old and new versions of the same data.

**All exports**
- R15. Large exports use bounded memory: memory use does not grow with the number of rows. The user sees progress as rows and size written.
- R16. The user can cancel an export at any time. A cancelled or failed export leaves no partial file where a finished one would be.
- R17. Exports only read data. They are allowed in every environment, including Production safe mode.
- R18. Excel exports respect the sheet row limit, by splitting across sheets or files, and tell the user that this happened.
- R19. Numbers that do not fit a spreadsheet or JSON number keep their exact value in the file.
- R20. File options stay minimal: delimiter and header row for delimited formats, and UTF-8 with an optional byte order mark so Excel opens Vietnamese and other non-ASCII text correctly.
- R21. Errors say what happened and what to do, with no raw driver output.

---

## Acceptance Examples

- AE1. **Covers R3, R4.** Given a query returned 10,000 rows and the grid holds 500, when the user opens result export, the dialog shows 500 rows on the grid, notes that the query returned more, and writing the file runs no query.
- AE2. **Covers R5.** Given a result from a join of two tables, when the user opens the format list, SQL Updates and Where Clause are disabled with a reason, and CSV and SQL Inserts remain available.
- AE3. **Covers R7, R8.** Given a result cut at the row limit from a SELECT, when the user chooses "Export all", the file ends up with every row the query returns. Given the statement is an UPDATE, "Export all" is refused with a reason.
- AE4. **Covers R9.** Given a query that uses `@total` set earlier in the tab, when the user chooses "Export all", the warning names session variables as a likely cause of different or failing results.
- AE5. **Covers R16.** Given an export of a large table is at 40% progress, when the user cancels, the export stops, the database query is cancelled, and no output file remains.
- AE6. **Covers R14.** Given a table receives writes while it is being exported, when the export finishes, no row in the file reflects a state from after the export began.
- AE7. **Covers R18.** Given a result of 1,500,000 rows exported to Excel, when the export finishes, the data is split across sheets or files within the row limit and the user is told.

---

## Success Criteria

- A user can take a table of tens of millions of rows to CSV or Parquet without the app slowing down or running out of memory, and can stop it at any moment.
- A user who exports a join result never believes a file is complete when it is not: row counts and any reduction are always visible.
- A user opens the CSV in Excel and sees correct Vietnamese text.
- A planner can start without inventing product behavior: formats per path, when each format is enabled, what the dialogs show, and what is out of scope are all stated here.

---

## Scope Boundaries

- Import of files into tables.
- Copy Table to another connection, Dump to DDL, and multi-table backups in the style of mysqldump.
- Server-side export (`SELECT ... INTO OUTFILE`, `COPY`).
- A mode that writes the file while the query runs for the first time.
- Several result sets from one script in one export.
- Raising the row limit of the grid.
- Resuming an export that was cancelled or lost its connection.
- Choosing columns, renaming headers and per-column formatting.
- Changes to the existing copy-as-clipboard actions: they stay as they are.

---

## Key Decisions

- Two paths instead of one: the grid result is small and already in hand, so it needs no streaming and can have a preview. A whole table or a full query result needs streaming and has no full preview.
- Result export reads the grid and does not run again by default: this removes the session-state, repeatable-read and double-run problems for the common case.
- "Export all" is an explicit, warned choice: the grid keeps only part of a large result, so more rows can come only from running the query again.
- Wider formats for table export, DataGrip-style set for result export: the DataGrip set serves developers and analysts well on small data. Parquet, NDJSON and TSV matter for large data, where table export is the path.
- Read-only exports are allowed in every environment: they cannot lose data.

---

## Dependencies / Assumptions

- The grid keeps a bounded number of rows per result and marks a result as cut when there are more (verified in the engine's run path, limit currently 500).
- Safe-mode statement classification already separates reads from writes and is reused to refuse non-read statements for "Export all".
- Grid edits already choose a key for a single source table; the same rule decides when SQL Updates and Where Clause are available (assumed reusable, to confirm in planning).
- The table total row count is available from the database without reading every row (to confirm per driver in planning).

---

## Outstanding Questions

### Deferred to Planning

- [Affects R14][Technical] How a consistent read is obtained on each supported driver, and what to do where it is not available.
- [Affects R15][Needs research] Which writer libraries meet the bounded-memory requirement for Excel and Parquet, and what batch sizes keep memory flat.
- [Affects R5, R10][Technical] How the source table of a result is determined, and when a result counts as coming from one table.
- [Affects R9][Technical] How reliably the statement text can reveal use of session variables or temporary tables.
- [Affects R13][Technical] Whether the total row count for a filtered table is exact or an estimate, and how it is labeled.
- [Affects R18][Technical] Whether Excel splits across sheets or across files, or lets the user choose.
- [Affects R2, R12][Technical] How the writers and previews share one code path, so the preview always matches the file.
