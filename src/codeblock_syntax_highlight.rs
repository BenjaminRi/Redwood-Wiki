use pulldown_cmark::{CodeBlockKind, CowStr, Event, Tag};

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;

use std::sync::OnceLock;

// To use the SyntaxHighlightStream, prior text merging is
// required to prevent confusing the syntect parser with
// events that only contain partial lines

pub struct SyntaxHighlightStream<'a, 'syn_set, I> {
	iter: I,
	inject_event: Option<Event<'a>>,
	html_generator: Option<ClassedHTMLGenerator<'syn_set>>,
}

impl<'a, 'syn_set, I> SyntaxHighlightStream<'a, 'syn_set, I>
where
	I: Iterator<Item = Event<'a>>,
{
	pub fn new(iter: I) -> Self {
		Self {
			iter,
			inject_event: None,
			html_generator: None,
		}
	}
}

impl<'a, 'syn_set, I> Iterator for SyntaxHighlightStream<'a, 'syn_set, I>
where
	I: Iterator<Item = Event<'a>>,
{
	type Item = Event<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		if self.inject_event.is_some() {
			let mut event = None;
			std::mem::swap(&mut event, &mut self.inject_event);
			return event;
		}

		match self.iter.next() {
			Some(Event::Start(Tag::CodeBlock(language))) => {
				static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
				let syntax_set: &SyntaxSet =
					SYNTAX_SET.get_or_init(|| SyntaxSet::load_defaults_newlines());

				let syntax = if let CodeBlockKind::Fenced(lang_str) = &language {
					syntax_set.find_syntax_by_token(&lang_str)
				} else {
					None
				}
				.unwrap_or_else(|| syntax_set.find_syntax_plain_text());

				self.html_generator = Some(ClassedHTMLGenerator::new_with_class_style(
					&syntax,
					syntax_set,
					ClassStyle::Spaced,
				));

				Some(Event::Start(Tag::CodeBlock(language)))
			}
			next_event @ Some(Event::End(Tag::CodeBlock(_))) => {
				let mut local_html_gen = None;
				std::mem::swap(&mut local_html_gen, &mut self.html_generator);
				// If the following `unwrap()` panics, it's a bug in `pulldown-cmark`,
				// because it means we had an `End` tag without a `Start` tag.
				let html = local_html_gen.unwrap().finalize();
				self.inject_event = next_event;
				Some(Event::Html(CowStr::Boxed(html.into_boxed_str())))
			}
			Some(Event::Text(text)) => {
				//println!("Text: {:?}", &text);

				if let Some(html_generator) = &mut self.html_generator {
					// We are in a highlighted code block
					html_generator
						.parse_html_for_line_which_includes_newline(&text)
						.unwrap();
					self.next()
				} else {
					// We are in a regular text element
					Some(Event::Text(text))
				}
			}
			event => event,
		}
	}
}
