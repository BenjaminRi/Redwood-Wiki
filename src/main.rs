use warp::Filter;

use chrono;

use rusqlite::{params, Connection, Result};

use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, Options, Parser, Tag};

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;

use std::sync::Arc;
use tokio::sync::Mutex;

use rusqlite::ToSql;

use std::collections::HashMap;

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

	fn get_article_title(&mut self, id: rowid) -> Option<String> {
		let mut stmt = self
			.conn
			.prepare("SELECT title FROM article WHERE id = ?")
			.unwrap();
		let mut article_iter = stmt.query_map(params![id], |row| Ok(row.get(0)?)).unwrap();

		if let Some(Ok(title)) = article_iter.next() {
			Some(title)
		} else {
			None
		}
	}

	fn update_article(&mut self, id: rowid, title: Option<&str>, text: Option<&str>) -> Result<usize, ()> {
		let mut query = "UPDATE article SET".to_string();

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

		arguments.push(Box::new(id.to_sql().unwrap()));
		query.push_str("WHERE id = ?");

		let updated = self.conn.execute(&query, &arguments[..]);

		if let Ok(updated) = updated {
			//println!("{} rows were updated", updated);
			Ok(updated)
		} else {
			//println!("failed");
			Err(())
		}
	}
}

fn rowid_from_str(link_str: &str) -> Option<rowid> {
	link_str
		.strip_prefix("id:")
		.map_or(None, |id_str| id_str.parse::<rowid>().ok())
}

fn expand_id_in_text(text: String, db: &mut Database) -> String {
	enum ParserState {
		Init,
		MatchPrefix1,        //i
		MatchPrefix2,        //id
		MatchRowid(Vec<u8>), //id:
	}

	//Add some additional capacity in case we do actually need to expand some IDs
	let mut str_buf: Vec<u8> = Vec::with_capacity(text.len() + 256);

	let mut parser_state = ParserState::Init;
	for ascii_char in text.bytes() {
		match parser_state {
			ParserState::Init => {
				if ascii_char != b'i' {
					str_buf.push(ascii_char);
				} else {
					parser_state = ParserState::MatchPrefix1;
				}
			}
			ParserState::MatchPrefix1 => {
				if ascii_char != b'd' {
					str_buf.extend_from_slice(b"i");
					str_buf.push(ascii_char);
					parser_state = ParserState::Init;
				} else {
					parser_state = ParserState::MatchPrefix2;
				}
			}
			ParserState::MatchPrefix2 => {
				if ascii_char != b':' {
					str_buf.extend_from_slice(b"id");
					str_buf.push(ascii_char);
					parser_state = ParserState::Init;
				} else {
					parser_state = ParserState::MatchRowid(Vec::with_capacity(32));
				}
			}
			ParserState::MatchRowid(mut id_buf) => {
				if ascii_char < b'0' || ascii_char > b'9' {
					if let Ok(id) = std::str::from_utf8(&id_buf).unwrap().parse::<rowid>() {
						let title = db
							.get_article_title(id)
							.or_else(|| Some("Unknown Article!".to_string()))
							.unwrap();
						str_buf.extend_from_slice(
							format!("<a href='../../article/{}'>{}</a>", id, title).as_bytes(),
						);
					} else {
						str_buf.extend_from_slice(b"id:");
						str_buf.extend_from_slice(&id_buf);
					}
					str_buf.push(ascii_char);
					parser_state = ParserState::Init;
				} else {
					id_buf.push(ascii_char);
					parser_state = ParserState::MatchRowid(id_buf);
				}
			}
		}
	}
	String::from_utf8(str_buf).unwrap() //Note: This should always work, otherwise it's a programmer error
}


// URL scheme: Suppose the wiki root is at `https://www.example.com/`
// Then article ID 5 could be accessed with
// `https://www.example.com/article/5/Title-of-fifth-article`
// The last part of the URL (`Title-of-fifth-article`) is purely cosmetic.
// The number (`5`) is the unique ID that is relevant for the database lookup.
// The article URL name is always encoded as `[id]/[title]` where the latter part is cosmetic.
// So it could also be accessed with
// `https://www.example.com/article/5`
// or
// `https://www.example.com/article/5/Foo`
// (these "wrong" titles might later trigger a redirect to the URL with the proper titles)
// Editing articles would be:
// `https://www.example.com/edit/article/1/Title-of-first-article`
// Previewing a pending edit would be
// `https://www.example.com/preview/article/1/Title-of-first-article`
// The idea is that the URL is always composed of
// `/[verb]/[item-type]/[item-id]`, except for plain showing articles, which can simply omit the verb.
// So, `/edit/article/1/Title-of-first-article` but `/article/1/Title-of-first-article` for showing.


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

	//db.update_article(4, Some("xA".to_string()), Some("xB".to_string()));

	//END SQLITE TEST

	let db = Arc::new(Mutex::new(db));
	let db = warp::any().map(move || db.clone());

	let index_path = warp::path::end().and(db.clone()).and_then(index_page);
	let article_path_post = warp::post()
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::param::<rowid>())
		.and(warp::path::end())
		.and(warp::body::form()) //This does not have a default size limit, it would be wise to use one to prevent a overly large request from using too much memory.
		//.and(warp::body::content_length_limit(1024 * 32))
		.and_then(article_page_post);
	let article_path_get = warp::get()
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::param::<rowid>())
		.and(warp::path::end())
		.and_then(article_page);
	let article_edit_path = warp::path("edit")
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::param::<rowid>())
		.and(warp::path::end())
		.and_then(article_edit_page);
	let routes = index_path.or(article_edit_path).or(article_path_get).or(article_path_post);
	warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn article_edit_page(
	db: Arc<Mutex<Database>>,
	article_number: rowid,
) -> Result<impl warp::Reply, warp::Rejection> {
	let mut db = db.lock().await;

	if let Some(article) = db.get_article(article_number) {
		let article_text = article.text;

		Ok(warp::reply::html(format!(
			r####"
<!DOCTYPE html>
<html>
	<head>
		<meta charset=utf-8>
		<meta name=viewport content="width=device-width, initial-scale=1.0">
		<meta name="description" content="">
		<title>Redwood-wiki</title>
		<style>
{}

{}
		</style>
	</head>
	<body>
		<link rel="stylesheet" href="https://unpkg.com/easymde/dist/easymde.min.css"> <!-- TODO: Self-host / hardcode this! -->
		<script src="https://unpkg.com/easymde/dist/easymde.min.js"></script> <!-- TODO: Self-host / hardcode this! -->
		
		<div class="main_content">
			<ul class="menu">
				<li><a href="/" class="menu_other">Home</a></li>
				<li><a href="../../article/{}/edit" class="menu_current">Edit</a></li>
			</ul>
			
			<h2>Redwood Wiki</h2>

			<p>Article {}</p>

			<p>
				<form action="../../article/{}" method="post">
					<label for="article_title">Title:</label><input type="text" id="article_title" name="article_title" class="editor_input"><br>
					<label for="article_text">Text:</label><br>
					<textarea id="article_text" name="article_text" class="editor_textarea">{}</textarea><br>
					<input type="submit" class="editor_submit">
				</form>
			</p>
			
			<script>
			var easyMDE = new EasyMDE({{
				lineNumbers: true,
				spellChecker: false,
				toolbar: false,
				element: document.getElementById('article_text')
			}});
			</script>

			<a href="../../article/1">go to article 1</a>
		</div>
	</body>
</html>
"####,
			GITHUB_MARKDOWN, MAIN_STYLE, article_number, article_number, article_number, article_text
		)))
	} else {
		Ok(warp::reply::html(format!(
			r####"
<!DOCTYPE html>
<html>
	<head>
		<meta charset=utf-8>
		<meta name=viewport content="width=device-width, initial-scale=1.0">
		<meta name="description" content="">
		<title>Redwood-wiki</title>
		<style>
{}

{}
		</style>
	</head>
	<body>
		<div class="main_content">
			<ul class="menu">
				<li><a href="/" class="menu_other">Home</a></li>
				<li><a href="../../article/{}/edit" class="menu_current">Edit</a></li>
			</ul>
			
			<h2>Redwood Wiki</h2>

			<p>Could not find article with id {}</p>
		</div>
	</body>
</html>
"####,
			GITHUB_MARKDOWN, MAIN_STYLE, article_number, article_number
		)))
	}
}

async fn article_page_post(
	db: Arc<Mutex<Database>>,
	article_number: rowid,
	param_map: HashMap<String, String>
) -> Result<impl warp::Reply, warp::Rejection> {
	
	{
		let mut db = db.lock().await;
		//println!("Post request: {:?}", param_map);
		db.update_article(article_number, param_map.get("article_title").map(|a| -> &str {a}), param_map.get("article_text").map(|a| -> &str {a})); //TODO: Two None parameters here lead to error, handle it
	}
	article_page(db, article_number).await
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

	let mut css_str = String::new();
	let ts = syntect::highlighting::ThemeSet::load_defaults();
	for (key, theme) in ts.themes {
		let css = syntect::html::css_for_theme_with_class_style(
			&theme,
			syntect::html::ClassStyle::Spaced,
		);
		//println!("{}.css - {}", key, css);
		css_str = css;
		break;
	}

	// Markdown handling
	let mut options = Options::empty();
	options.insert(Options::ENABLE_STRIKETHROUGH);

	let syntax_set = SyntaxSet::load_defaults_newlines();
	let mut html_generator: Option<ClassedHTMLGenerator> = None;

	let parser = Parser::new_ext(&article_text, options).map(|event| {
		//println!("Text: {:?}", &event);
		match event {
			Event::Start(Tag::CodeBlock(language)) => {
				let mut syntax = if let CodeBlockKind::Fenced(lang_str) = &language {
					syntax_set.find_syntax_by_token(&lang_str)
				} else {
					None
				}
				.unwrap_or_else(|| syntax_set.find_syntax_plain_text());

				html_generator = Some(ClassedHTMLGenerator::new_with_class_style(
					&syntax,
					&syntax_set,
					ClassStyle::Spaced,
				));

				Event::Start(Tag::CodeBlock(language))
			}
			Event::Start(Tag::Link(link_type, mut dest_url, title)) => {
				let url_str: &str = &dest_url;
				if let Some(id) = rowid_from_str(url_str) {
					dest_url = CowStr::Boxed(
						("../../article/".to_owned() + &id.to_string()).into_boxed_str(),
					);
				}
				Event::Start(Tag::Link(link_type, dest_url, title))
			}
			Event::End(Tag::CodeBlock(_)) => {
				let mut local_html_gen = None;
				std::mem::swap(&mut local_html_gen, &mut html_generator);
				let mut html = local_html_gen.unwrap().finalize(); // If this panics, it's a bug in `pulldown-cmark`
				html.push_str("</code></pre>");
				Event::Html(CowStr::Boxed(html.into_boxed_str()))
			}
			Event::Text(text) => {
				//println!("Text: {:?}", &text);

				if let Some(html_generator) = &mut html_generator {
					// We are in a highlighted code block
					html_generator.parse_html_for_line_which_includes_newline(&text);
					Event::Text(CowStr::Borrowed(""))
				} else {
					// We are in a regular text element
					Event::Text(text)
				}
			}
			_ => event,
		}
	});

	//let parser = Parser::new_ext(&article_text, options);

	// Write to String buffer.
	let mut html_output = String::new();
	html::push_html(&mut html_output, parser);

	html_output = expand_id_in_text(html_output, &mut db);

	Ok(warp::reply::html(format!(
		r####"
<!DOCTYPE html>
<html>
	<head>
		<meta charset=utf-8>
		<meta name=viewport content="width=device-width, initial-scale=1.0">
		<meta name="description" content="">
		<title>Redwood-wiki</title>
		<style>
{}

{}

{}
		</style>
	</head>
	<body>
		<div class="main_content">
			<ul class="menu">
				<li><a href="/" class="menu_other">Home</a></li>
				<li><a href="../../edit/article/{}" class="menu_current">Edit</a></li>
			</ul>
			
			<h2>Redwood Wiki</h2>

			<p>Article {}</p>

			<p>Text:</p>

			<p>{}</p>

			<a href="../../article/1">go to article 1</a>
		</div>
	</body>
</html>
"####,
		css_str, GITHUB_MARKDOWN, MAIN_STYLE, article_number, article_number, html_output
	)))
}

//<div contenteditable="true"></div>
//<style type=text/css>body { max-width: 800px; margin: auto; }</style>

async fn index_page(db: Arc<Mutex<Database>>) -> Result<impl warp::Reply, warp::Rejection> {
	let db = db.lock().await;
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

const MAIN_STYLE: &str = include_str!("css/main_style.css");
const GITHUB_MARKDOWN: &str = include_str!("css/github_markdown.css");
