use warp::Filter;

use chrono;
use chrono::Utc;

use pulldown_cmark::{html, CodeBlockKind, CowStr, Event, Options, Parser, Tag};

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;

use warp::Reply;

use std::sync::Arc;
use tokio::sync::Mutex;

use std::collections::HashMap;
use std::path::Path;

mod database;

use database::Article;
use database::Database;
use database::DatabaseConnection;
use database::Rowid;

//https://blog.joco.dev/posts/warp_auth_server_tutorial

fn rowid_from_str(link_str: &str) -> Option<Rowid> {
	link_str
		.strip_prefix("id:")
		.map_or(None, |id_str| id_str.parse::<Rowid>().ok())
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
					if let Ok(id) = std::str::from_utf8(&id_buf).unwrap().parse::<Rowid>() {
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

#[tokio::main(flavor = "current_thread")]
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

	let db = DatabaseConnection::new(Path::new("./test.sqlite"), database::OpenMode::OpenOrCreate)
		.unwrap()
		.init();

	let db = Arc::new(Mutex::new(db));
	let db = warp::any().map(move || db.clone());

	let index_path = warp::path::end().and(db.clone()).and_then(index_page);
	let article_path_post = warp::post()
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::param::<Rowid>())
		.and(warp::path::end())
		.and(warp::body::form()) //This does not have a default size limit, it would be wise to use one to prevent a overly large request from using too much memory.
		//.and(warp::body::content_length_limit(1024 * 32))
		.and_then(article_page_post);
	let article_path_get = warp::get()
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::param::<Rowid>())
		.and(warp::path::end())
		.and_then(article_page);
	let article_edit_path = warp::path("edit")
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::param::<Rowid>())
		.and(warp::path::end())
		.and_then(article_edit_page);
	let article_create_get_path = warp::get()
		.and(warp::path("create"))
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::end())
		.and_then(article_create_page);
	let article_create_post_path = warp::post()
		.and(warp::path("create"))
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::end())
		.and(warp::body::form()) //This does not have a default size limit, it would be wise to use one to prevent a overly large request from using too much memory.
		//.and(warp::body::content_length_limit(1024 * 32))
		.and_then(article_create_page_post);
	let routes = index_path
		.or(article_edit_path)
		.or(article_path_get)
		.or(article_path_post)
		.or(article_create_get_path)
		.or(article_create_post_path);
	warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn article_edit_page(
	db: Arc<Mutex<Database>>,
	article_number: Rowid,
) -> Result<impl warp::Reply, warp::Rejection> {
	let mut db = db.lock().await;

	if let Some(article) = db.get_article(article_number) {
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
		<script>
			{}
		</script>
		
		
		{}
		<div class="main_content">
			<div class="content markdown">
				<ul class="menu">
					<li><a href="../../preview/article/{}" class="menu_current">Preview</a></li>
				</ul>

				<p>Article {}</p>

				<p>
					<form action="../../article/{}" method="post">
						<label for="article_title">Title:</label><input type="text" id="article_title" name="article_title" class="editor_input" value="{}"><br>
						<label for="article_text">Text:</label><br>
						<textarea id="article_text" name="article_text" class="editor_textarea">{}</textarea><br>
						<input type="submit" class="editor_submit" value="Save">
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
			</div>
		</div>
	</body>
</html>
"####,
			GITHUB_MARKDOWN,
			MAIN_STYLE,
			include_str!("easymde/easymde.min.css"),
			include_str!("easymde/easymde.min.js"),
			generate_menu(Some(article_number)),
			article_number,
			article_number,
			article_number,
			&article.title,
			&article.text
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
		{}
		<div class="main_content">
			<div class="content markdown">
				<ul class="menu">
				</ul>

				<p>Could not find article with id {}</p>
			</div>
		</div>
	</body>
</html>
"####,
			GITHUB_MARKDOWN,
			MAIN_STYLE,
			generate_menu(Some(article_number)),
			article_number
		)))
	}
}

async fn article_page_post(
	db: Arc<Mutex<Database>>,
	article_number: Rowid,
	param_map: HashMap<String, String>,
) -> Result<impl warp::Reply, warp::Rejection> {
	{
		let mut db = db.lock().await;
		log::trace!("Article update post request: {:?}", param_map);
		db.update_article(
			article_number,
			param_map.get("article_title").map(|a| -> &str { a }),
			param_map.get("article_text").map(|a| -> &str { a }),
		); //TODO: Two None parameters here lead to error, handle it
	}
	article_page(db, article_number).await
}

async fn article_page(
	db: Arc<Mutex<Database>>,
	article_number: Rowid,
) -> Result<impl warp::Reply, warp::Rejection> {
	let mut db = db.lock().await;

	if let Some(article) = db.get_article(article_number) {
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

		/*let mut callback = |link: pulldown_cmark::BrokenLink<'_>| {
			println!("{:?}", link.reference);
			Some((CowStr::Boxed("a".to_owned().into_boxed_str()), CowStr::Boxed("b".to_owned().into_boxed_str())))
		};

		let parser = Parser::new_with_broken_link_callback(&article.text, options, Some(&mut callback)).map(|event| {*/

		let parser = Parser::new_ext(&article.text, options).map(|event| {
			//println!("Text: {:?}", &event);
			match event {
				Event::Start(Tag::CodeBlock(language)) => {
					let syntax = if let CodeBlockKind::Fenced(lang_str) = &language {
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

		//let parser = Parser::new_ext(&article.text, options);

		// Write to String buffer.
		let mut html_output = String::new();
		html::push_html(&mut html_output, parser);

		html_output = expand_id_in_text(html_output, &mut db);

		if html_output == "" {
			html_output = format!("[This article is empty. Click <a href='../../edit/article/{}'>here</a> to edit it.]", article.id);
		}

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
		{}
		<div class="main_content">
			<div class="content markdown">
				<h1>{} <span style="color: #BBBBBB;">#{}</span></h1>

				{}
				
			</div>
		</div>
	</body>
</html>
"####,
			css_str,
			GITHUB_MARKDOWN,
			MAIN_STYLE,
			generate_menu(Some(article_number)),
			&article.title,
			article_number,
			html_output
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
		{}
		<div class="main_content">
			<div class="content markdown">
				<p>Could not find article #{}!</p>
			</div>
		</div>
	</body>
</html>
"####,
			GITHUB_MARKDOWN,
			MAIN_STYLE,
			generate_menu(Some(article_number)),
			article_number
		)))
	}
}

//<div contenteditable="true"></div>
//<style type=text/css>body { max-width: 800px; margin: auto; }</style>

async fn index_page(db: Arc<Mutex<Database>>) -> Result<impl warp::Reply, warp::Rejection> {
	let db = db.lock().await;
	Ok(warp::reply::html(format!(
		r#"
<!DOCTYPE html>
<html>
	<head>
		<meta charset=utf-8>
		<meta name=viewport content="width=device-width, initial-scale=1.0">
		<meta name="description" content="">
		<title>Redwood-wiki</title>
		<style>
{}
		</style>
	</head>
	<body>
		{}
		<div class="main_content">
			<div class="content markdown">
				<h2 style="margin-top: 0px;">Redwood Wiki</h2>
			</div>
		</div>
	</body>
</html>
	"#,
		MAIN_STYLE,
		generate_menu(None)
	)))
}

async fn article_create_page_post(
	db: Arc<Mutex<Database>>,
	param_map: HashMap<String, String>,
) -> Result<impl warp::Reply, warp::Rejection> {
	let mut db = db.lock().await;

	log::trace!("Article create post request: {:?}", param_map);

	let art = Article {
		id: 0,
		title: param_map.get("article_title").unwrap().to_string(), //TODO: Dangerous unwrap here, can crash server!
		text: "".to_string(),
		date_created: Utc::now().naive_utc(),
		date_modified: Utc::now().naive_utc(),
		revision: 0,
	};

	let create_result = db.create_article(&art);
	if let Some(id) = create_result {
		Ok(
			warp::redirect(warp::http::Uri::from_maybe_shared(format!("/article/{}", id)).unwrap())
				.into_response(),
		)
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
		</style>
	</head>
	<body>
		{}
		<div class="main_content">
			<div class="content markdown">
				<p>
					Could not create article. Title already existing?
				</p>
			</div>
		</div>
	</body>
</html>
"####,
			MAIN_STYLE,
			generate_menu(None)
		))
		.into_response())
	}
}

async fn article_create_page(
	_db: Arc<Mutex<Database>>,
) -> Result<impl warp::Reply, warp::Rejection> {
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
		</style>
	</head>
	<body>
		{}
		<div class="main_content">
			<div class="content markdown">
				<p>
					<form action="/create/article" method="post">
						<label for="article_title">Title:</label><input type="text" id="article_title" name="article_title" class="editor_input" value="">
						<input type="submit" class="editor_submit" value="Create">
					</form>
				</p>
			</div>
		</div>
	</body>
</html>
"####,
		MAIN_STYLE,
		generate_menu(None)
	)))
}

fn generate_menu(article_number_opt: Option<Rowid>) -> String {
	if let Some(article_number) = article_number_opt {
		format!(
			r#"<div class="side_content">
			<div class="content">
				{} Redwood wiki
				<p>
					Navigation:
					<ul>
						<li><a href="/">Home</a></li>
					</ul>
				</p>
				<p>
					Wiki:
					<ul>
						<li><a href="/create/article">Create article</a></li>
					</ul>
				</p>
				<p>
					Current article:
					<ul>
						<li><a href="/edit/article/{}">Edit</a></li>
					</ul>
				</p>
			</div>
		</div>"#,
			REDWOOD_OBS, article_number
		)
	} else {
		format!(
			r#"<div class="side_content">
			<div class="content">
				{} Redwood wiki
				<p>
					Navigation:
					<ul>
						<li><a href="/">Home</a></li>
					</ul>
				</p>
				<p>
					Wiki:
					<ul>
						<li><a href="/create/article">Create article</a></li>
					</ul>
				</p>
			</div>
		</div>"#,
			REDWOOD_OBS
		)
	}
}

const MAIN_STYLE: &str = include_str!("css/main_style.css");
const GITHUB_MARKDOWN: &str = include_str!("css/github_markdown.css");

const REDWOOD_OBS: &str = r#"<img style="width: 112px; height: 112px;", src="data:image/png;base64, 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" alt="Redwood Observatory" />
"#;
