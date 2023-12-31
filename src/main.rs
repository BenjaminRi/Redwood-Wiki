use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use chrono;
use chrono::Utc;

use pulldown_cmark::{html, CowStr, Event, LinkType, Options, Parser, Tag};

use tokio::sync::Mutex;

use warp::{Filter, Reply};

mod database;
use database::{Article, Database, DatabaseConnection, ItemId};

mod config;
use config::parse_config;

mod markdown_utils;
use markdown_utils::{LinkHighlightStream, TextMergeStream, UnknownRefHandlingStream};

mod codeblock_syntax_highlight;
use codeblock_syntax_highlight::SyntaxHighlightStream;

mod regex_utils;
use regex::RegexBuilder;
use regex_utils::{DoPartition, Part};

struct HtmlDocument {
	title: String,
	style: String,
	styles: Vec<&'static str>,
	scripts: Vec<&'static str>,
	body: String,
}
impl HtmlDocument {
	fn new() -> HtmlDocument {
		HtmlDocument {
			title: "Redwood-wiki".to_string(),
			style: String::new(),
			styles: vec![],
			scripts: vec![],
			body: String::new(),
		}
	}

	fn to_html(&self) -> String {
		format!(
			r####"
<!DOCTYPE html>
<html>
	<head>
		<meta charset=utf-8>
		<meta name=viewport content="width=device-width, initial-scale=1.0">
		<meta name="description" content="">
		<title>{}</title>
		<link rel="icon" href="/favicon.ico" sizes="any"><!-- 32×32 -->
		<link rel="icon" href="/icon.svg" type="image/svg+xml">
		<style>
{}

{}
		</style>
		<script>
{}
		</script>
	</head>
	<body>
		{}
	</body>
</html>
"####,
			self.title,
			self.style,
			self.styles.join("\n\n"),
			self.scripts.join("\n\n"),
			self.body,
		)
	}
}

//https://blog.joco.dev/posts/warp_auth_server_tutorial

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

	let config = parse_config().unwrap();

	let db = DatabaseConnection::new(
		&config.database.storage_location.join("wiki_db.sqlite"),
		database::OpenMode::OpenOrCreate,
	)
	.unwrap()
	.init()
	.unwrap();

	let db = Arc::new(Mutex::new(db));
	let db = warp::any().map(move || db.clone());

	let index_path = warp::path::end().and(db.clone()).and_then(index_page);
	let favicon_ico_path_get = warp::get()
		.and(warp::path("favicon.ico"))
		.and(warp::path::end())
		.and_then(favicon_ico_page);
	let favicon_svg_path_get = warp::get()
		.and(warp::path("icon.svg"))
		.and(warp::path::end())
		.and_then(favicon_svg_page);
	let wiki_icon_page_get = warp::get()
		.and(warp::path("img"))
		.and(warp::path("redwood_observatory_medium.png"))
		.and(warp::path::end())
		.and_then(wiki_icon_page);
	let article_path_post = warp::post()
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::param::<ItemId>())
		.and(warp::path::end())
		.and(warp::body::form()) //This does not have a default size limit, it would be wise to use one to prevent a overly large request from using too much memory.
		//.and(warp::body::content_length_limit(1024 * 32))
		.and_then(article_page_post);
	let article_path_get = warp::get()
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::param::<ItemId>())
		.and(warp::path::end())
		.and_then(article_page);
	let search_path_post = warp::post()
		.and(warp::path("search"))
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::end())
		.and(warp::body::form()) //This does not have a default size limit, it would be wise to use one to prevent a overly large request from using too much memory.
		//.and(warp::body::content_length_limit(1024 * 32))
		.and_then(search_page_post);
	let search_path_get = warp::get()
		.and(warp::path("search"))
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::end())
		.and_then(search_page_get);
	let article_edit_path = warp::path("edit")
		.and(warp::path("article"))
		.and(db.clone())
		.and(warp::path::param::<ItemId>())
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
	let articles_path = warp::get()
		.and(warp::path("articles"))
		.and(db.clone())
		.and(warp::path::end())
		.and_then(articles_page);
	let routes = index_path
		.or(favicon_ico_path_get)
		.or(favicon_svg_path_get)
		.or(wiki_icon_page_get)
		.or(article_edit_path)
		.or(article_path_get)
		.or(article_path_post)
		.or(search_path_get)
		.or(search_path_post)
		.or(article_create_get_path)
		.or(article_create_post_path)
		.or(articles_path);
	warp::serve(routes)
		.run((config.network.ip, config.network.port))
		.await;
}

async fn article_edit_page(
	db: Arc<Mutex<Database>>,
	article_number: ItemId,
) -> Result<impl warp::Reply, warp::Rejection> {
	let mut db = db.lock().await;

	if let Some(article) = db.get_article(article_number) {
		let mut doc = HtmlDocument::new();
		doc.styles.push(GITHUB_MARKDOWN);
		doc.styles.push(MAIN_STYLE);
		doc.styles.push(include_str!("easymde/easymde.min.css"));
		doc.scripts.push(include_str!("easymde/easymde.min.js"));
		doc.body = format!(
			r####"
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
					autoDownloadFontAwesome: false,
					lineNumbers: true,
					spellChecker: false,
					toolbar: false,
					element: document.getElementById('article_text')
				}});
				</script>
			</div>
		</div>
"####,
			generate_menu(Some(article_number)),
			article_number,
			article_number,
			article_number,
			&article.title,
			&article.text
		);
		Ok(warp::reply::html(doc.to_html()))
	} else {
		let mut doc = HtmlDocument::new();
		doc.styles.push(GITHUB_MARKDOWN);
		doc.styles.push(MAIN_STYLE);
		doc.body = format!(
			r####"
		{}
		<div class="main_content">
			<div class="content markdown">
				<ul class="menu">
				</ul>

				<p>Could not find article with id {}</p>
			</div>
		</div>
"####,
			generate_menu(None),
			article_number
		);
		Ok(warp::reply::html(doc.to_html()))
	}
}

async fn article_page_post(
	db: Arc<Mutex<Database>>,
	article_number: ItemId,
	param_map: HashMap<String, String>,
) -> Result<impl warp::Reply, warp::Rejection> {
	{
		let mut db = db.lock().await;
		log::trace!("Article update post request: {:?}", param_map);
		db.update_article(
			article_number,
			param_map.get("article_title").map(|a| -> &str { a }),
			param_map.get("article_text").map(|a| -> &str { a }),
		)
		.unwrap(); //TODO: Two None parameters here lead to error, handle it
	}
	article_page(db, article_number).await
}

fn handle_unknown_ref<'a>(
	db: &mut Database,
	inject_event: &mut VecDeque<Event<'a>>,
	_link_url: &str,
	_link_title: &str,
	link_text: &str,
) {
	//println!("Unknown ref: {} {} {}", link_url, link_title, link_text);
	if let Some(article_str) = link_text.strip_prefix("article:") {
		let mut article_iter = article_str.split('|');

		if let Some(id_str) = article_iter.next() {
			if let Ok(id) = id_str.parse::<ItemId>() {
				let dest_url = "../../article/".to_owned() + id_str;
				if let Some(title) = db.get_article_title(id) {
					let displayed_title = article_iter
						.next()
						.map_or_else(|| title.to_string(), |s| s.to_string());
					inject_event.push_back(Event::Start(Tag::Link(
						LinkType::Autolink,
						CowStr::Boxed(dest_url.to_string().into_boxed_str()),
						CowStr::Boxed(title.to_string().into_boxed_str()),
					)));
					inject_event
						.push_back(Event::Text(CowStr::Boxed(displayed_title.into_boxed_str())));
					inject_event.push_back(Event::End(Tag::Link(
						LinkType::Autolink,
						CowStr::Boxed(dest_url.to_string().into_boxed_str()),
						CowStr::Boxed(title.to_string().into_boxed_str()),
					)));
					return;
				}
			}
		} else {
			unreachable!();
		}
	}

	// Does not match any wiki commands... Just emit as text.
	inject_event.push_back(Event::Text(CowStr::Boxed(
		format!("[{}]", link_text).into_boxed_str(),
	)));
}

async fn article_page(
	db: Arc<Mutex<Database>>,
	article_number: ItemId,
) -> Result<impl warp::Reply, warp::Rejection> {
	let mut db = db.lock().await;

	if let Some(article) = db.get_article(article_number) {
		let mut css_str = String::new();
		let ts = syntect::highlighting::ThemeSet::load_defaults();
		for (_key, theme) in ts.themes {
			let css = syntect::html::css_for_theme_with_class_style(
				&theme,
				syntect::html::ClassStyle::Spaced,
			)
			.unwrap();
			//println!("{}.css - {}", _key, css);
			css_str = css;
			break;
		}

		// Markdown handling
		let mut options = Options::empty();
		options.insert(Options::ENABLE_TABLES); // https://www.tablesgenerator.com/markdown_tables
										//options.insert(Options::ENABLE_FOOTNOTES); // https://www.markdownguide.org/extended-syntax/#footnotes
		options.insert(Options::ENABLE_STRIKETHROUGH); // `~~strikethrough~~`
		options.insert(Options::ENABLE_TASKLISTS); // `- [ ]` or `- [x]` or `- [X]`
										   //options.insert(Options::ENABLE_SMART_PUNCTUATION); // creates em-dashes for `--` and nice quotes for `"Hello."` or `'thing'`
										   //For smart punctuation, also see spec: https://github.com/raphlinus/pulldown-cmark/blob/d99667b3a8843744494366799025dcea614ff866/third_party/CommonMark/smart_punct.txt

		let mut broken_link_callback = |_link: pulldown_cmark::BrokenLink<'_>| {
			//println!("{:?}", link.reference);

			// Returns Option<link_url, hover_description>
			// Because we need deeper modifications (in particular, the
			// text of the link itself, we just return empty strings here
			// and modify the ShortcutUnknown Link events.
			Some((CowStr::Borrowed(""), CowStr::Borrowed("")))
		};

		let mut unknown_ref_callback = |inject_event: &mut VecDeque<Event>,
		                                link_url: &str,
		                                link_title: &str,
		                                link_text: &str| {
			// Capture the locked database `db` in the closure here
			handle_unknown_ref(&mut db, inject_event, link_url, link_title, link_text);
		};

		let parser = UnknownRefHandlingStream::new(
			TextMergeStream::new(Parser::new_with_broken_link_callback(
				&article.text,
				options,
				Some(&mut broken_link_callback),
			)),
			&mut unknown_ref_callback,
		);

		let parser = LinkHighlightStream::new(SyntaxHighlightStream::new(parser.into_iter()));

		// Write to String buffer.
		let mut html_output = String::new();
		html::push_html(&mut html_output, parser);

		if html_output == "" {
			html_output = format!("[This article is empty. Click <a href='../../edit/article/{}'>here</a> to edit it.]", article.id);
		}

		let mut doc = HtmlDocument::new();
		doc.style = css_str;
		doc.styles.push(GITHUB_MARKDOWN);
		doc.styles.push(MAIN_STYLE);
		doc.body = format!(
			r####"
		{}
		<div class="main_content">
			<div class="content markdown">
				<h1>{} <span style="color: #BBBBBB;">#{}</span> <a href='../../edit/article/{}'>[edit]</a></h1>

				{}
				
			</div>
		</div>
"####,
			generate_menu(Some(article_number)),
			&article.title,
			article_number,
			article_number,
			html_output
		);
		Ok(warp::reply::html(doc.to_html()))
	} else {
		let mut doc = HtmlDocument::new();
		doc.styles.push(GITHUB_MARKDOWN);
		doc.styles.push(MAIN_STYLE);
		doc.body = format!(
			r####"
		{}
		<div class="main_content">
			<div class="content markdown">
				<p>Could not find article #{}!</p>
			</div>
		</div>
"####,
			generate_menu(None),
			article_number
		);
		Ok(warp::reply::html(doc.to_html()))
	}
}

async fn search_page_post(
	db: Arc<Mutex<Database>>,
	param_map: HashMap<String, String>,
) -> Result<impl warp::Reply, warp::Rejection> {
	let mut db = db.lock().await;
	log::trace!("Article update post request: {:?}", param_map);

	let empty_string = String::new();
	let search_term = param_map.get("search_term_plain").unwrap_or(&empty_string);
	let articles = db.search_articles(search_term);

	if let Some(articles) = articles {
		use std::fmt::Write;
		let search_regex = RegexBuilder::new(&regex::escape(search_term))
			.case_insensitive(true)
			.build()
			.expect("Invalid Regex");

		let mut exact_list_html = "<br>\nExact matches:<br>\n".to_string();
		let mut title_list_html = "<br>\nTitle matches:<br>\n".to_string();
		let mut text_list_html = "<br>\nText matches:<br>\n".to_string();
		let mut exact_match_cnt = 0;
		let mut title_match_cnt = 0;
		let mut text_match_cnt = 0;
		for article in &articles {
			let mut title_match = false;
			let mut title = String::new();
			for part in search_regex.partition(&article.title) {
				match part {
					Part::NoMatch(text) => {
						Write::write_str(&mut title, text).unwrap();
					}
					Part::Match(text) => {
						title_match = true;
						write!(title, "<b style=\"color:red;\">{}</b>", text).unwrap();
					}
				}
			}

			//TODO: Unify with generate_articles_list elsewhere. Have one unique way to show article lists.

			if article.title.to_lowercase() == search_term.to_lowercase() {
				exact_match_cnt += 1;
				write!(
					exact_list_html,
					"<a href=\"/article/{}\">{}</a> <span style=\"color: #BBBBBB;\">#{}</span><br>\n",
					article.id, title, article.id
				)
				.unwrap();
			} else if title_match {
				title_match_cnt += 1;
				write!(
					title_list_html,
					"<a href=\"/article/{}\">{}</a> <span style=\"color: #BBBBBB;\">#{}</span><br>\n",
					article.id, title, article.id
				)
				.unwrap();
			} else {
				text_match_cnt += 1;
				write!(
					text_list_html,
					"<a href=\"/article/{}\">{}</a> <span style=\"color: #BBBBBB;\">#{}</span><br>\n",
					article.id, title, article.id
				)
				.unwrap();
			}
			//log::info!("{:?}", title);
			//titles.push_str(&format!("<a href=\"https://foo\">{}</a>", title));
		}

		if exact_match_cnt == 0 {
			exact_list_html.clear();
		}

		if title_match_cnt == 0 {
			title_list_html.clear();
		}

		if text_match_cnt == 0 {
			text_list_html.clear();
		}

		let mut doc = HtmlDocument::new();
		doc.styles.push(MAIN_STYLE);
		doc.body = format!(
			r#"
		{}
		<div class="main_content">
			<div class="content markdown">
				<h2 style="margin-top: 0px;">Articles</h2>
				<p>
				{}{}{}
				</p>
			</div>
		</div>
"#,
			generate_menu(None),
			exact_list_html,
			title_list_html,
			text_list_html
		);
		Ok(warp::reply::html(doc.to_html()))
	} else {
		let mut doc = HtmlDocument::new();
		doc.styles.push(MAIN_STYLE);
		doc.body = format!(
			r#"
		{}
		<div class="main_content">
			<div class="content markdown">
				<p>
					Could not fetch the articles.
				</p>
			</div>
		</div>
"#,
			generate_menu(None)
		);
		Ok(warp::reply::html(doc.to_html()))
	}
}

async fn search_page_get(db: Arc<Mutex<Database>>) -> Result<impl warp::Reply, warp::Rejection> {
	let mut _db = db.lock().await;
	//TODO: Add search page
	Ok(warp::reply::html(format!(
		"Search page not yet implemented"
	)))
}

//<div contenteditable="true"></div>
//<style type=text/css>body { max-width: 800px; margin: auto; }</style>

async fn index_page(db: Arc<Mutex<Database>>) -> Result<impl warp::Reply, warp::Rejection> {
	let _db = db.lock().await;
	let mut doc = HtmlDocument::new();
	doc.styles.push(MAIN_STYLE);
	doc.body = format!(
		r#"
		{}
		<div class="main_content">
			<div class="content markdown">
				<h2 style="margin-top: 0px;">Redwood Wiki</h2>
			</div>
		</div>
"#,
		generate_menu(None)
	);
	Ok(warp::reply::html(doc.to_html()))
}

// TODO: Perhaps use the following crate?
// https://stackoverflow.com/questions/77257503/serve-static-files-in-warp-that-are-bundled-in-the-executable
// https://crates.io/crates/static_dir
// Pictures are only cached with
// Last-Modified (https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Last-Modified) or
// ETag (https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/ETag)

use warp::http::response::Response;

const FAVICON_ICO: &'static [u8] = include_bytes!("favicon/favicon.ico");

async fn favicon_ico_page() -> Result<impl warp::Reply, warp::Rejection> {
	let response = Response::builder()
		.status(200)
		.header("Content-Type", "image/x-icon")
		.body(FAVICON_ICO)
		.unwrap();
	Ok(response)
}

const FAVICON_SVG: &'static [u8] = include_bytes!("favicon/icon.svg");

async fn favicon_svg_page() -> Result<impl warp::Reply, warp::Rejection> {
	let response = Response::builder()
		.status(200)
		.header("Content-Type", "image/svg+xml")
		.body(FAVICON_SVG)
		.unwrap();
	Ok(response)
}

const WIKI_ICON: &'static [u8] = include_bytes!("wiki_icon/redwood_observatory_medium.png");

async fn wiki_icon_page() -> Result<impl warp::Reply, warp::Rejection> {
	let response = Response::builder()
		.status(200)
		.header("Content-Type", "image/png")
		.header("last-modified", "Sun, 31 Dec 2023 16:36:16 GMT") // TODO: Get last time modified from actual file
		.body(WIKI_ICON)
		.unwrap();
	Ok(response)
}

async fn articles_page(db: Arc<Mutex<Database>>) -> Result<impl warp::Reply, warp::Rejection> {
	let mut db = db.lock().await;
	let articles = db.get_all_articles();

	fn generate_articles_list(articles: Vec<Article>) -> String {
		let mut accumulator = String::new();
		for article in &articles {
			use std::fmt::Write;
			write!(
				accumulator,
				"<a href=\"/article/{}\">{}</a> <span style=\"color: #BBBBBB;\">#{}</span><br>\n",
				article.id, article.title, article.id
			)
			.unwrap();
		}
		accumulator
	}

	if let Some(articles) = articles {
		let mut doc = HtmlDocument::new();
		doc.styles.push(MAIN_STYLE);
		doc.body = format!(
			r#"
		{}
		<div class="main_content">
			<div class="content markdown">
				<h2 style="margin-top: 0px;">Articles</h2>
				<p>
				{}
				</p>
			</div>
		</div>
"#,
			generate_menu(None),
			generate_articles_list(articles)
		);
		Ok(warp::reply::html(doc.to_html()))
	} else {
		let mut doc = HtmlDocument::new();
		doc.styles.push(MAIN_STYLE);
		doc.body = format!(
			r#"
		{}
		<div class="main_content">
			<div class="content markdown">
				<p>
					Could not fetch the articles.
				</p>
			</div>
		</div>
"#,
			generate_menu(None)
		);
		Ok(warp::reply::html(doc.to_html()))
	}
}

async fn article_create_page_post(
	db: Arc<Mutex<Database>>,
	param_map: HashMap<String, String>,
) -> Result<impl warp::Reply, warp::Rejection> {
	let mut db = db.lock().await;

	log::trace!("Article create post request: {:?}", param_map);

	let art = Article {
		id: 0.into(),
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
		let mut doc = HtmlDocument::new();
		doc.styles.push(MAIN_STYLE);
		doc.body = format!(
			r####"
		{}
		<div class="main_content">
			<div class="content markdown">
				<p>
					Could not create article. Title already existing?
				</p>
			</div>
		</div>
"####,
			generate_menu(None)
		);
		Ok(warp::reply::html(doc.to_html()).into_response())
	}
}

async fn article_create_page(
	_db: Arc<Mutex<Database>>,
) -> Result<impl warp::Reply, warp::Rejection> {
	let mut doc = HtmlDocument::new();
	doc.styles.push(MAIN_STYLE);
	doc.body = format!(
		r####"
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
"####,
		generate_menu(None)
	);
	Ok(warp::reply::html(doc.to_html()))
}

fn generate_menu(article_number_opt: Option<ItemId>) -> String {
	if let Some(article_number) = article_number_opt {
		format!(
			r#"<div class="side_content">
			<div class="content">
				{} Redwood wiki
				<p>
					Search:
					<form action="/search/article" method="post">
						<input type="text" id="search_term_plain" name="search_term_plain" value=""><input type="submit" class="editor_submit" value="Search">
					</form>
				</p>
				<p>
					Navigation:
					<ul>
						<li><a href="/">Home</a></li>
						<li><a href="/articles">All articles</a></li>
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
					Search:
					<form action="/search/article" method="post">
						<input type="text" id="search_term_plain" name="search_term_plain" value=""><input type="submit" class="editor_submit" value="Search">
					</form>
				</p>
				<p>
					Navigation:
					<ul>
						<li><a href="/">Home</a></li>
						<li><a href="/articles">All articles</a></li>
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

const REDWOOD_OBS: &str = r#"<img style="width: 112px; height: 112px;" src="/img/redwood_observatory_medium.png" alt="Redwood Observatory">"#;
