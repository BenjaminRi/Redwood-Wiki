use std::path::Path;

use chrono;
use chrono::Utc;

use std::convert::TryFrom;

use rusqlite::{
	params, types::FromSql, types::FromSqlError, types::ToSqlOutput, types::ValueRef, Connection,
	OpenFlags, ToSql,
};

#[derive(Debug, Copy, Clone)]
pub struct ItemId {
	value: u32,
}

impl From<u32> for ItemId {
	fn from(value: u32) -> Self {
		ItemId { value }
	}
}

impl TryFrom<i64> for ItemId {
	type Error = std::num::TryFromIntError;

	fn try_from(value: i64) -> Result<Self, Self::Error> {
		u32::try_from(value).map(|value| ItemId { value })
	}
}

fn parse_u32(in_str: &str) -> Option<u32> {
	if in_str.starts_with("+") {
		// Do not accept strings like `+10`
		// We don't want versions like `+1.+3.+9`
		None
	} else {
		in_str.parse::<u32>().ok()
	}
}

impl std::str::FromStr for ItemId {
	type Err = ();
	fn from_str(in_str: &str) -> Result<Self, <Self as std::str::FromStr>::Err> {
		parse_u32(in_str).map(|value| ItemId { value }).ok_or(())
	}
}

impl std::fmt::Display for ItemId {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
		write!(f, "{}", self.value)
	}
}

impl FromSql for ItemId {
	fn column_result(val_ref: ValueRef<'_>) -> Result<Self, FromSqlError> {
		let value = val_ref.as_i64()?;
		u32::try_from(value)
			.map(|value| ItemId { value })
			.map_err(|_| rusqlite::types::FromSqlError::InvalidType)
	}
}

impl ToSql for ItemId {
	fn to_sql(&self) -> Result<ToSqlOutput<'_>, rusqlite::Error> {
		self.value.to_sql()
	}
}

#[derive(Debug)]
pub struct Article {
	pub id: ItemId,
	pub title: String,
	pub text: String,
	pub date_created: chrono::NaiveDateTime,
	pub date_modified: chrono::NaiveDateTime,
	pub revision: i64,
}

#[derive(Debug, std::cmp::PartialEq)]
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
	fn column_result(
		val_ref: rusqlite::types::ValueRef<'_>,
	) -> rusqlite::types::FromSqlResult<Self> {
		let version_str = val_ref.as_str()?;
		let mut version_iter = version_str.split('.');

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
	id: ItemId,
	version: WikiSemVer,
	migrating_to_version: Option<WikiSemVer>,
	date_created: chrono::NaiveDateTime,
	date_migration_begin: Option<chrono::NaiveDateTime>,
	date_migration_complete: Option<chrono::NaiveDateTime>,
}

pub struct Database {
	conn: rusqlite::Connection,
}

#[allow(dead_code)]
pub enum OpenMode {
	CreateNew,
	OpenExisting,
	OpenOrCreate,
}

#[derive(Debug)]
pub enum DatabaseConnectError {
	AlreadyExists,
	CannotOpen,
	CouldNotCreate,
	Unknown,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum DatabaseInitError {
	CouldNotReadLayout,
	UnsupportedLayout,
	MigrationNeeded,
}

impl From<rusqlite::Error> for DatabaseConnectError {
	fn from(sqlite_error: rusqlite::Error) -> DatabaseConnectError {
		log::error!("SQLite error: {:?}", sqlite_error);
		if let rusqlite::Error::SqliteFailure(inner, _) = sqlite_error {
			match inner.code {
				rusqlite::ErrorCode::CannotOpen => DatabaseConnectError::CannotOpen,
				_ => DatabaseConnectError::Unknown,
			}
		} else {
			DatabaseConnectError::Unknown
		}
	}
}

pub struct DatabaseConnection {
	database: Database,
}

impl DatabaseConnection {
	pub fn new(
		database_path: &Path,
		open_mode: OpenMode,
	) -> Result<DatabaseConnection, DatabaseConnectError> {
		log::info!("SQLite version: {}", rusqlite::version());

		pub fn create_new(
			database_path: &Path,
		) -> Result<DatabaseConnection, DatabaseConnectError> {
			// Note: Here, SQLite forces us to open the database
			// with a racy file exists check. The reason for that
			// is that the `SQLITE_OPEN_EXCLUSIVE ` flag is not yet
			// available. However, it will most likely be present
			// in future releases, allowing a race condition free
			// database initialization. More details here:
			// https://sqlite.org/forum/forumpost/680cd395b4bc97c6

			if database_path.exists() {
				// TODO: Instead of exists check, use `SQLITE_OPEN_EXCLUSIVE`
				// (see comment above)
				return Err(DatabaseConnectError::AlreadyExists);
			}

			let conn_result = Connection::open_with_flags(
				database_path,
				OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
			);

			if let Err(rusqlite::Error::SqliteFailure(
				rusqlite::ffi::Error {
					code: rusqlite::ErrorCode::CannotOpen,
					..
				},
				_,
			)) = &conn_result
			{
				// We can unwrap_err here because we know it is an error.
				log::error!("SQLite error: {:?}", conn_result.unwrap_err());
				// SQLite returns "CannotOpen" when it can't create
				// the database file.
				// This is why we need a special error code mapping here.
				// The reason for this SQLite behavior is unknown.
				return Err(DatabaseConnectError::CouldNotCreate);
			}

			let conn = conn_result?;
			let mut database = Database { conn };
			database.init_tables();
			let dbc = DatabaseConnection { database };
			Ok(dbc)
		}

		pub fn open_existing(
			database_path: &Path,
		) -> Result<DatabaseConnection, DatabaseConnectError> {
			let conn =
				Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;

			let database = Database { conn };
			let dbc = DatabaseConnection { database };
			return Ok(dbc);
		}

		let dbc = match open_mode {
			OpenMode::CreateNew => create_new(database_path),
			OpenMode::OpenExisting => open_existing(database_path),
			OpenMode::OpenOrCreate => {
				// Note: This check is racy, but once `create_new`
				// becomes atomic, the worst consequence is that we try
				// to create a database that already exists and fail
				// without causing any harm or undefined states.
				if database_path.exists() {
					open_existing(database_path)
				} else {
					create_new(database_path)
				}
			}
		}?;

		Ok(dbc)
	}

	pub fn init(mut self) -> Result<Database, DatabaseInitError> {
		let layout = self.database.get_table_layout();
		if let Some(layout) = layout {
			if layout.version
				== (WikiSemVer {
					major: 0,
					minor: 1,
					patch: 0,
				}) {
				Ok(self.database)
			} else {
				Err(DatabaseInitError::UnsupportedLayout)
			}
		} else {
			Err(DatabaseInitError::CouldNotReadLayout)
		}
	}
}

impl Database {
	/// Filter illegal characters that
	/// shall be prevented from entering the
	/// database
	fn filter_chars(s: &str) -> String {
		//TODO: Optimize with std::borrow::Cow<str>?
		s.replace(
			&[
				'\u{00}', // NUL
				'\u{01}', // SOH
				'\u{02}', // STX
				'\u{03}', // ETX
				'\u{04}', // EOT
				'\u{05}', // ENQ
				'\u{06}', // ACK
				'\u{07}', // BEL
				'\u{08}', // BS
				//'\u{09}', // TAB (allowed)
				//'\u{0A}', // LF (allowed)
				'\u{0B}', // VT
				'\u{0C}', // FF
				'\u{0D}', // CR
				'\u{0E}', // S0
				'\u{0F}', // SI
				'\u{10}', // DLE
				'\u{11}', // DC1
				'\u{12}', // DC2
				'\u{13}', // DC3
				'\u{14}', // DC4
				'\u{15}', // NAK
				'\u{16}', // SYN
				'\u{17}', // ETB
				'\u{18}', // CAN
				'\u{19}', // EM
				'\u{1A}', // SUB
				'\u{1B}', // ESC
				'\u{1C}', // FS
				'\u{1D}', // GS
				'\u{1E}', // RS
				'\u{1F}', // US
			][..],
			"",
		)
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
			id: 1.into(),
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

	pub fn create_article(&mut self, article: &Article) -> Option<ItemId> {
		let now = Utc::now().naive_utc();
		if article.title.is_empty() {
			None
		} else if let Ok(1) = self.conn
			.execute(
				"INSERT INTO article (title, text, date_created, date_modified, revision) VALUES (?1, ?2, ?3, ?4, ?5)",
				params![Database::filter_chars(&article.title), Database::filter_chars(&article.text), now, now, article.revision],
			) {
			ItemId::try_from(self.conn.last_insert_rowid()).ok()
		} else {
			None
		}
	}

	#[allow(dead_code)]
	pub fn test_tables(&mut self) {
		let art1 = Article {
			id: 0.into(),
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

	pub fn get_article(&mut self, id: ItemId) -> Option<Article> {
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
	/// Get all existing articles
	///
	/// Note: This dumps the entire database. Depending on the
	/// number of articles, this might be slow and use a lot of RAM
	pub fn get_all_articles(&mut self) -> Option<Vec<Article>> {
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

		let mut articles = Vec::new();
		for article in article_iter {
			articles.push(article.unwrap());
		}

		Some(articles)
	}

	/// Search article
	pub fn search_articles(&mut self, search_term: &str) -> Option<Vec<Article>> {
		let search_term = format!(
			"%{}%",
			str::replace(search_term, "^", "^^")
				.replace("%", "^%")
				.replace("_", "^_")
		);
		let mut stmt = self
			.conn
			.prepare("SELECT id, title, text, date_created, date_modified, revision FROM article WHERE title LIKE ? ESCAPE '^' OR text LIKE ? ESCAPE '^'")
			.unwrap();
		let article_iter = stmt
			.query_map(params![search_term, search_term], |row| {
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

		let mut articles = Vec::new();
		for article in article_iter {
			articles.push(article.unwrap());
		}

		Some(articles)
	}

	pub fn get_article_title(&mut self, id: ItemId) -> Option<String> {
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
		id: ItemId,
		title: Option<&str>,
		text: Option<&str>,
	) -> Result<usize, ()> {
		let title = title.map(|s| Database::filter_chars(s));
		let text = text.map(|s| Database::filter_chars(s));

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

		match self
			.conn
			.execute(&query, rusqlite::params_from_iter(arguments.iter()))
		{
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
