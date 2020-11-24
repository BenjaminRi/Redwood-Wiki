use warp::Filter;

use chrono;

use rusqlite::{params, Connection, Result};

use pulldown_cmark::{html, Options, Parser};

use std::sync::Arc;
use tokio::sync::Mutex;

use i64 as rowid;

#[derive(Debug)]
struct Article {
	id: rowid,
	title: String,
	text: String,
}

//https://blog.joco.dev/posts/warp_auth_server_tutorial

struct Database {
	conn: rusqlite::Connection,
}

impl Database {
	fn init_tables(&mut self) {
		self.conn
			.execute(
				"CREATE TABLE IF NOT EXISTS article (
					id     INTEGER PRIMARY KEY AUTOINCREMENT,
					title  TEXT NOT NULL,
					text   TEXT NOT NULL
				)",
				params![],
			)
			.unwrap();
	}

	fn test_tables(&mut self) {
		let art1 = Article {
			id: 0,
			title: "TITLE_x".to_string(),
			text: "TEXT_x".to_string(),
		};
		self.conn
			.execute(
				"INSERT INTO article (title, text) VALUES (?1, ?2)",
				params![art1.title, art1.text],
			)
			.unwrap();
		let mut stmt = self
			.conn
			.prepare("SELECT id, title, text FROM article")
			.unwrap();
		let article_iter = stmt
			.query_map(params![], |row| {
				Ok(Article {
					id: row.get(0)?,
					title: row.get(1)?,
					text: row.get(2)?,
				})
			})
			.unwrap();

		for article in article_iter {
			println!("Found article {:?}", article.unwrap());
		}
	}

	fn get_article(&mut self, id: rowid) -> Option<Article> {
		let mut stmt = self
			.conn
			.prepare("SELECT id, title, text FROM article WHERE id = ?")
			.unwrap();
		let mut article_iter = stmt
			.query_map(params![id], |row| {
				Ok(Article {
					id: row.get(0)?,
					title: row.get(1)?,
					text: row.get(2)?,
				})
			})
			.unwrap();

		if let Some(Ok(article)) = article_iter.next() {
			Some(article)
		} else {
			None
		}
	}
}

#[tokio::main]
async fn main() {
	fern::Dispatch::new()
		// Perform allocation-free log formatting
		.format(|out, message, record| {
			out.finish(format_args!(
				"{}[{}][{}] {}",
				chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
				record.target(),
				record.level(),
				message
			))
		})
		.level(log::LevelFilter::Warn)
		.level_for("redwood_wiki", log::LevelFilter::Trace)
		.chain(std::io::stdout())
		//.chain(fern::log_file("output.log").unwrap())
		// Apply globally
		.apply()
		.unwrap();

	log::info!("Starting Redwood-Wiki!");

	//SQLITE TEST

	let conn = Connection::open("test.sqlite").unwrap();
	let mut db = Database { conn };
	db.init_tables();
	//db.test_tables();

	//END SQLITE TEST

	let db = Arc::new(Mutex::new(db));
	let db = warp::any().map(move || db.clone());

	let index_path = warp::path::end().and(db.clone()).and_then(index_page);
	let article_path = warp::path("article")
		.and(db.clone())
		.and(warp::path::param::<rowid>())
		.and_then(article_page);
	let routes = index_path.or(article_path);
	warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn article_page(
	db: Arc<Mutex<Database>>,
	article_number: rowid,
) -> Result<impl warp::Reply, warp::Rejection> {
	let mut db = db.lock().await;

	let article_text = {
		if let Some(article) = db.get_article(article_number) {
			article.text
		} else {
			"".to_string()
		}
	};

	// Markdown handling
	let mut options = Options::empty();
	options.insert(Options::ENABLE_STRIKETHROUGH);
	let parser = Parser::new_ext(&article_text, options);

	// Write to String buffer.
	let mut html_output = String::new();
	html::push_html(&mut html_output, parser);

	Ok(warp::reply::html(format!(
		r#"
<!DOCTYPE html>
<html>
<body>

<h2>Redwood Wiki</h2>

<p>Article {}</p>

<p>Text:</p>

<p>{}</p>

</body>
</html>	
	"#,
		article_number, html_output
	)))
}

async fn index_page(db: Arc<Mutex<Database>>) -> Result<impl warp::Reply, warp::Rejection> {
	let mut db = db.lock().await;
	Ok(warp::reply::html(
		r#"
<!DOCTYPE html>
<html>
<body>

<h2>Redwood Wiki</h2>

<p>Welcome to Redwood Wiki!</p>

<p>Articles:</p>

<p>Users:</p>

</body>
</html>	
	"#,
	))
}
