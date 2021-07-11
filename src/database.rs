use i64 as rowid;

use std::path::Path;

use chrono;
use chrono::Utc;

use rusqlite::{params, types::FromSql, types::ToSqlOutput, Connection, OpenFlags, ToSql};

#[derive(Debug)]
pub struct Article {
	pub id: rowid,
	pub title: String,
	pub text: String,
	pub date_created: chrono::NaiveDateTime,
	pub date_modified: chrono::NaiveDateTime,
	pub revision: i64,
}

#[derive(Debug)]
pub struct WikiSemVer {
	major: u32,
	minor: u32,
	patch: u32,
}

impl ToSql for WikiSemVer {
	fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
		Ok(ToSqlOutput::Owned(rusqlite::types::Value::Text(format!(
			"{}.{}.{}",
			self.major, self.minor, self.patch
		))))
	}
}

impl FromSql for WikiSemVer {
	fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
		let version_str = value.as_str()?;
		let mut version_iter = version_str.split('.');

		fn parse_u32(in_str: &str) -> Option<u32> {
			if in_str.starts_with("+") {
				// Do not accept strings like `+10`
				// We don't want versions like `+1.+3.+9`
				None
			} else {
				in_str.parse::<u32>().ok()
			}
		}

		match (
			version_iter.next(),
			version_iter.next(),
			version_iter.next(),
		) {
			(Some(major), Some(minor), Some(patch)) => {
				match (parse_u32(major), parse_u32(minor), parse_u32(patch)) {
					(Some(major), Some(minor), Some(patch)) => Ok(WikiSemVer {
						major,
						minor,
						patch,
					}),
					_ => Err(rusqlite::types::FromSqlError::InvalidType), // Could not parse individual slices into u32
				}
			}
			_ => Err(rusqlite::types::FromSqlError::InvalidType), // Could not slice version into 3-tuple
		}
	}
}

#[derive(Debug)]
pub struct TableLayout {
	id: rowid,
	version: WikiSemVer,
	migrating_to_version: Option<WikiSemVer>,
	date_created: chrono::NaiveDateTime,
	date_migration_begin: Option<chrono::NaiveDateTime>,
	date_migration_complete: Option<chrono::NaiveDateTime>,
}

pub struct Database {
	conn: rusqlite::Connection,
}

pub enum OpenMode {
	CreateNew,
	OpenExisting,
	OpenOrCreate,
}

impl Database {
	pub fn new(database_path: &Path, open_mode: OpenMode) -> Database {
		// TODO: open_mode is not used, its enum variants aren't implemented.
		// TODO: Return Result<Database, Error> rather than Database here.
		let init_needed = !database_path.exists();
		let conn = Connection::open_with_flags(
			database_path,
			OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
		)
		.unwrap();

		let mut db = Database { conn };

		if init_needed {
			db.init_tables();
		}
		db
	}

	pub fn init_tables(&mut self) {
		// Note: SQLite does not have a DATETIME type
		// Therefore, we implement datetime types as
		// TEXT with ISO 8601 format.

		self.conn
			.execute(
				"CREATE TABLE article (
					id            INTEGER PRIMARY KEY AUTOINCREMENT,
					title         TEXT NOT NULL UNIQUE,
					text          TEXT NOT NULL,
					date_created  DATETIME NOT NULL,
					date_modified DATETIME NOT NULL,
					revision      INTEGER NOT NULL
				)",
				params![],
			)
			.unwrap();

		// The following table MUST only ever have one row.
		// Note that this table MUST always be present and
		// MUST NOT ever change its layout.

		// The versions are stored as strings. They follow a
		// simplified semver ( https://semver.org/ ) format
		// that only allows a three-tuple of u32 integers
		// (e.g. 1.1.9 is allowed, 1.1.9-alpha is not)

		// `version` is a version that indicates the
		// current state of the table layout.
		// Semver rules apply when opening the database.
		// Note that the format version is not necessarily
		// the same as the active `redwood-wiki` version.

		// `migrating_to_version` is NULL during normal operation.
		// During database migration, it contains the format version
		// we migrate to. Incomplete or cancelled database migrations
		// can be detected with the help of this field. After the
		// migration, the field must be reset to NULL again.

		// When a migration starts, `date_migration_begin` is set
		// to the current timestamp and `date_migration_complete` is
		// set to NULL. Additionally, `migrating_to_version` is set
		// to the version we migrate to. These three modifications
		// must be done atomically in the same transaction.

		// When the migration is complete, `date_migration_complete` is set
		// to the current timestamp and `date_migration_begin` is left
		// untouched. Additionally, `migrating_to_version` is set
		// to NULL again, atomically in the same transaction.

		self.conn
			.execute(
				"CREATE TABLE table_layout (
					id                        INTEGER PRIMARY KEY AUTOINCREMENT,
					version                   TEXT NOT NULL,
					migrating_to_version      TEXT,
					date_created              DATETIME NOT NULL,
					date_migration_begin      DATETIME,
					date_migration_complete   DATETIME
				)",
				params![],
			)
			.unwrap();

		let layout = TableLayout {
			id: 1,
			version: WikiSemVer {
				major: 0,
				minor: 1,
				patch: 0,
			},
			migrating_to_version: None,
			date_created: Utc::now().naive_utc(),
			date_migration_begin: None,
			date_migration_complete: None,
		};

		self.conn
			.execute(
				"INSERT INTO table_layout (id, version, migrating_to_version, date_created, date_migration_begin, date_migration_complete) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
				params![layout.id, layout.version, layout.migrating_to_version, layout.date_created, layout.date_migration_begin, layout.date_migration_complete],
			)
			.unwrap();

		log::debug!("Table layout: {:?}", self.get_table_layout());
	}

	pub fn get_table_layout(&mut self) -> Option<TableLayout> {
		let mut stmt = self
			.conn
			.prepare(
				"SELECT id, version, migrating_to_version, date_created, date_migration_begin, date_migration_complete FROM table_layout WHERE id = ?",
			)
			.unwrap();
		let mut table_layout_iter = stmt
			.query_map(params![1], |row| {
				Ok(TableLayout {
					id: row.get(0)?,
					version: row.get(1)?,
					migrating_to_version: row.get(2)?,
					date_created: row.get(3)?,
					date_migration_begin: row.get(4)?,
					date_migration_complete: row.get(5)?,
				})
			})
			.unwrap();

		if let Some(table_layout_result) = table_layout_iter.next() {
			match table_layout_result {
				Ok(table_layout) => Some(table_layout),
				Err(err) => {
					log::error!("Could not parse table layout: {:?}", err);
					None
				}
			}
		} else {
			log::error!("Could not find table layout!");
			None
		}
	}

	pub fn create_article(&mut self, article: &Article) -> Option<rowid> {
		let now = Utc::now().naive_utc();
		if let Ok(1) = self.conn
			.execute(
				"INSERT INTO article (title, text, date_created, date_modified, revision) VALUES (?1, ?2, ?3, ?4, ?5)",
				params![article.title, article.text, now, now, article.revision],
			) {
			Some(self.conn.last_insert_rowid())
		} else {
			None
		}
	}

	#[allow(dead_code)]
	pub fn test_tables(&mut self) {
		let art1 = Article {
			id: 0,
			title: "TITLE_x".to_string(),
			text: "TEXT_x".to_string(),
			date_created: Utc::now().naive_utc(),
			date_modified: Utc::now().naive_utc(),
			revision: 0,
		};
		self.conn
			.execute(
				"INSERT INTO article (title, text) VALUES (?1, ?2)",
				params![art1.title, art1.text],
			)
			.unwrap();
		let mut stmt = self
			.conn
			.prepare("SELECT id, title, text, date_created, date_modified, revision FROM article")
			.unwrap();
		let article_iter = stmt
			.query_map(params![], |row| {
				Ok(Article {
					id: row.get(0)?,
					title: row.get(1)?,
					text: row.get(2)?,
					date_created: row.get(3)?,
					date_modified: row.get(4)?,
					revision: row.get(5)?,
				})
			})
			.unwrap();

		for article in article_iter {
			log::debug!("Found article {:?}", article.unwrap());
		}
	}

	pub fn get_article(&mut self, id: rowid) -> Option<Article> {
		let mut stmt = self
			.conn
			.prepare(
				"SELECT id, title, text, date_created, date_modified, revision FROM article WHERE id = ?",
			)
			.unwrap();
		let mut article_iter = stmt
			.query_map(params![id], |row| {
				Ok(Article {
					id: row.get(0)?,
					title: row.get(1)?,
					text: row.get(2)?,
					date_created: row.get(3)?,
					date_modified: row.get(4)?,
					revision: row.get(5)?,
				})
			})
			.unwrap();

		if let Some(article_result) = article_iter.next() {
			match article_result {
				Ok(article) => Some(article),
				Err(err) => {
					log::error!("Could not parse article: {:?}", err);
					None
				}
			}
		} else {
			log::debug!("Could not find article with id {}", id);
			None
		}
	}

	pub fn get_article_title(&mut self, id: rowid) -> Option<String> {
		let mut stmt = self
			.conn
			.prepare("SELECT title FROM article WHERE id = ?")
			.unwrap();
		let mut article_iter = stmt.query_map(params![id], |row| Ok(row.get(0)?)).unwrap();

		if let Some(Ok(title)) = article_iter.next() {
			Some(title)
		} else {
			log::debug!("Could not get tile for article with id {}", id);
			None
		}
	}

	pub fn update_article(
		&mut self,
		id: rowid,
		title: Option<&str>,
		text: Option<&str>,
	) -> Result<usize, ()> {
		let mut query = "UPDATE article SET".to_string();

		let now = Utc::now().naive_utc();
		let mut arguments: Vec<Box<dyn rusqlite::ToSql>> = vec![];

		let mut need_delim = false;
		let delim = ',';

		for (param, sql_str) in &[(&title, " title = ? "), (&text, " text = ? ")] {
			if let Some(param) = param {
				// Only update the SQL column if parameter is not None
				// otherwise let it keep its original value
				arguments.push(Box::new(param.to_sql().unwrap()));
				if need_delim {
					query.push(delim);
				} else {
					need_delim = true
				}
				query.push_str(sql_str);
			}
		}

		arguments.push(Box::new(now.to_sql().unwrap()));
		query.push(delim);
		query.push_str(" date_modified = ? ");

		query.push(delim);
		query.push_str(" revision = revision + 1 ");

		arguments.push(Box::new(id.to_sql().unwrap()));
		query.push_str("WHERE id = ?");

		match self.conn.execute(&query, &arguments[..]) {
			Ok(updated) => {
				log::debug!("Article update: {} row successfully updated", updated);
				Ok(updated)
			}
			Err(err) => {
				log::error!("Article update failed: {:?}", err);
				Err(())
			}
		}
	}
}
