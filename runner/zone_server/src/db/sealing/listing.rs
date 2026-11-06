use std::collections::BTreeSet;

const COMMENT: char = ';';
const DATABASE_HEADER: &str = "dbname:";
const TABLE_DATA: [&str; 3] = ["TABLE", "DATA", "public"];

/// What `pg_restore --list` says an archive holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Listing {
    database: String,
    tables: BTreeSet<String>,
}

impl Listing {
    /// Parse the table of contents `pg_restore --list` printed.
    pub fn parse(listing: &str) -> Self {
        let mut database = String::new();
        let mut tables = BTreeSet::new();
        for line in listing.lines() {
            if let Some(comment) = line.strip_prefix(COMMENT) {
                if let Some(name) = comment.trim().strip_prefix(DATABASE_HEADER) {
                    database = name.trim().to_string();
                }
                continue;
            }
            let tokens: Vec<&str> = line.split_whitespace().collect();
            tables.extend(
                tokens
                    .windows(TABLE_DATA.len() + 1)
                    .filter_map(|window| window.split_last())
                    .filter(|(_, kind)| *kind == TABLE_DATA)
                    .map(|(table, _)| table.to_string()),
            );
        }
        Self { database, tables }
    }

    /// The database the archive was dumped from.
    pub fn database(&self) -> &str {
        &self.database
    }

    /// Whether the archive holds the rows of `public.<table>`.
    pub fn holds_data_for(&self, table: &str) -> bool {
        self.tables.contains(table)
    }
}

#[cfg(test)]
mod tests {
    use super::Listing;

    const LISTING: &str = ";
; Archive created at 2026-11-06 20:25:50 NZDT
;     dbname: zone_test
;     TOC Entries: 529
;     Compression: gzip
;     Dump Version: 1.16-0
;     Format: CUSTOM
;     Integer: 4 bytes
;     Offset: 8 bytes
;     Dumped from database version: 17.11 (Debian 17.11-1.pgdg12+2)
;     Dumped by pg_dump version: 18.6 (Homebrew)
;
;
; Selected TOC Entries:
;
2; 3079 16393 EXTENSION - vector 
4739; 0 0 COMMENT - EXTENSION vector 
219; 1259 16740 TABLE public agent_logins postgres
248; 1259 16992 TABLE public projects postgres
257; 1259 17070 TABLE public sources postgres
259; 1259 17089 TABLE public sync_configs postgres
4664; 0 16385 TABLE DATA public _sqlx_migrations postgres
4693; 0 16992 TABLE DATA public projects postgres
4702; 0 17070 TABLE DATA public sources postgres
4704; 0 17089 TABLE DATA public sync_configs postgres
4740; 0 0 SEQUENCE SET public chat_entries_position_seq postgres
4053; 2606 17322 CONSTRAINT public agent_logins agent_logins_pkey postgres
4518; 2620 17653 TRIGGER public sync_configs sync_configs_updated_at postgres
4469; 2606 17984 FK CONSTRAINT public sync_configs sync_configs_project_id_fkey postgres
";

    #[test]
    fn the_header_names_the_database() {
        assert_eq!(Listing::parse(LISTING).database(), "zone_test");
    }

    #[test]
    fn only_table_data_entries_count_as_holding_a_table() {
        let listing = Listing::parse(LISTING);
        for table in ["projects", "sources", "sync_configs", "_sqlx_migrations"] {
            assert!(listing.holds_data_for(table), "{table}");
        }
        assert!(
            !listing.holds_data_for("agent_logins"),
            "a table whose data was excluded still has schema entries"
        );
        assert!(!listing.holds_data_for("project"));
    }

    #[test]
    fn a_commented_out_entry_holds_nothing() {
        let listing = Listing::parse(";4693; 0 16992 TABLE DATA public projects postgres\n");
        assert!(!listing.holds_data_for("projects"));
        assert_eq!(listing.database(), "");
    }
}
