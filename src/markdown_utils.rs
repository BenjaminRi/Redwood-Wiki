use std::collections::VecDeque;

use regex::Regex;
use std::sync::OnceLock;

use pulldown_cmark::{CowStr, Event, LinkType, Tag};

use super::regex_utils::{DoPartition, Part};

// Merge text events together. More information at:
// https://github.com/raphlinus/pulldown-cmark/issues/507

pub struct TextMergeStream<'a, I> {
	iter: I,
	last_event: Option<Event<'a>>,
}

impl<'a, I> TextMergeStream<'a, I>
where
	I: Iterator<Item = Event<'a>>,
{
	pub fn new(iter: I) -> Self {
		Self {
			iter,
			last_event: None,
		}
	}
}

impl<'a, I> Iterator for TextMergeStream<'a, I>
where
	I: Iterator<Item = Event<'a>>,
{
	type Item = Event<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		match (self.last_event.take(), self.iter.next()) {
			(Some(Event::Text(last_text)), Some(Event::Text(next_text))) => {
				// We need to start merging consecutive text events together into one
				let mut string_buf: String = last_text.into_string();
				string_buf.push_str(&next_text);
				loop {
					// Avoid recursion to avoid stack overflow and to optimize concatenation
					match self.iter.next() {
						Some(Event::Text(next_text)) => {
							string_buf.push_str(&next_text);
						}
						next_event => {
							self.last_event = next_event;
							if string_buf.is_empty() {
								// Discard text event(s) altogether if there is no text
								break self.next();
							} else {
								break Some(Event::Text(CowStr::Boxed(
									string_buf.into_boxed_str(),
								)));
							}
						}
					}
				}
			}
			(None, Some(next_event)) => {
				// This only happens once during the first iteration and if there are items
				self.last_event = Some(next_event);
				self.next()
			}
			(None, None) => {
				// This happens when the iterator is depleted
				None
			}
			(last_event, next_event) => {
				// The ordinary case, emit one event after the other without modification
				self.last_event = next_event;
				last_event
			}
		}
	}
}

// To use the LinkHighlightStream, prior text merging is
// required to prevent link text events being sliced up:
//<a href="https://url.com/foo">https://url.com/foo</a>[bar
//<a href="https://url.com/foo">https://url.com/foo</a>]bar
//<a href="https://url.com/foo">https://url.com/foo</a>*bar

pub struct LinkHighlightStream<'a, I> {
	iter: I,
	inject_event: VecDeque<Event<'a>>,
	inside_link: bool,
}

impl<'a, I> LinkHighlightStream<'a, I>
where
	I: Iterator<Item = Event<'a>>,
{
	pub fn new(iter: I) -> Self {
		Self {
			iter,
			inject_event: VecDeque::new(),
			inside_link: false,
		}
	}
}

impl<'a, I> Iterator for LinkHighlightStream<'a, I>
where
	I: Iterator<Item = Event<'a>>,
{
	type Item = Event<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		if !self.inject_event.is_empty() {
			return self.inject_event.pop_front();
		}

		if self.inside_link {
			// Suspend link detection logic within certain elements like autolinks
			// to avoid breaking or duplicating links.
			return self.iter.next();
		}

		match self.iter.next() {
			Some(Event::Text(next_text)) => {
				// We found a text event, apply link replacement
				// Note: This is inefficient in two ways:
				// 1. If the regex does not match, we could just straight emit the event
				//    and skip all this vector and to_string() stuff altogether.
				// 2. We could skip the VecDeque collect(), pop_front(), etc. entirely if we
				//    could solve the lifetime problem of keeping the Partition iterator around

				// Regex to find links: Characters taken from
				// https://www.ietf.org/rfc/rfc3986.txt
				// Section 2.2. Reserved Characters
				// Section 2.3. Unreserved Characters
				// A-Za-z0-9-_.~:/?#[]@!$&'()*+,;=

				static LINK_REGEX: OnceLock<Regex> = OnceLock::new();
				let link_regex: &Regex = LINK_REGEX.get_or_init(|| {
					Regex::new(
						r"(?P<p>https?)://(?P<l>[A-Za-z0-9\-_\.\~:/\?\#\[\]@!\$\&'\(\)\*\+,;=]+)",
					)
					.unwrap()
				});

				self.inject_event = link_regex
					.partition(&next_text)
					.flat_map(|mat| match mat {
						Part::NoMatch(text) => vec![Event::Text(CowStr::Boxed(
							text.to_string().into_boxed_str(),
						))]
						.into_iter(),
						Part::Match(text) => vec![
							Event::Start(Tag::Link(
								LinkType::Autolink,
								CowStr::Boxed(text.to_string().into_boxed_str()),
								CowStr::Borrowed(""),
							)),
							Event::Text(CowStr::Boxed(text.to_string().into_boxed_str())),
							Event::End(Tag::Link(
								LinkType::Autolink,
								CowStr::Boxed(text.to_string().into_boxed_str()),
								CowStr::Borrowed(""),
							)),
						]
						.into_iter(),
					})
					.collect();
				self.next()
			}
			next_event @ Some(Event::Start(Tag::Link(_, _, _))) => {
				self.inside_link = true;
				next_event
			}
			next_event @ Some(Event::End(Tag::Link(_, _, _))) => {
				self.inside_link = false;
				next_event
			}
			next_event => next_event,
		}
	}
}

pub type UnknownRefCallback<'a, 'b> = &'b mut dyn FnMut(&mut VecDeque<Event<'a>>, &str, &str, &str);

pub struct UnknownRefHandlingStream<'a, 'b, I> {
	iter: I,
	inject_event: VecDeque<Event<'a>>,
	ref_handler: UnknownRefCallback<'a, 'b>,
}

impl<'a, 'b, 'c, I> UnknownRefHandlingStream<'a, 'b, I>
where
	I: Iterator<Item = Event<'a>>,
{
	pub fn new(iter: I, ref_handler: UnknownRefCallback<'a, 'b>) -> Self {
		Self {
			iter,
			inject_event: VecDeque::new(),
			ref_handler,
		}
	}
}

impl<'a, 'b, I> Iterator for UnknownRefHandlingStream<'a, 'b, I>
where
	I: Iterator<Item = Event<'a>>,
{
	type Item = Event<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		if !self.inject_event.is_empty() {
			return self.inject_event.pop_front();
		}

		match self.iter.next() {
			Some(Event::Start(Tag::Link(LinkType::ShortcutUnknown, link_url, link_title))) => {
				match self.iter.next() {
					Some(Event::Text(text)) => {
						// Link text found
						(self.ref_handler)(&mut self.inject_event, &link_url, &link_title, &text);
					}
					Some(Event::End(Tag::Link(LinkType::ShortcutUnknown, _, _))) => {
						// No link text? Link end without any contents??
					}
					_ => {
						// No link text? No link end? Ignore stray event.
					}
				}
				loop {
					match self.iter.next() {
						Some(Event::End(Tag::Link(LinkType::ShortcutUnknown, _, _))) => {
							break;
						}
						None => {
							break;
						}
						_ => {
							// Ignore all other events between start and end
							continue;
						}
					}
				}
				self.next()
			}
			next_evt => next_evt,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_text_merge() {
		assert_eq!(
			TextMergeStream::new(vec![].into_iter()).collect::<Vec<Event<'_>>>(),
			vec![]
		);
		assert_eq!(
			TextMergeStream::new(vec![Event::Text(CowStr::Borrowed("foo"))].into_iter())
				.collect::<Vec<Event<'_>>>(),
			vec![Event::Text(CowStr::Borrowed("foo"))]
		);
		assert_eq!(
			TextMergeStream::new(
				vec![
					Event::Text(CowStr::Borrowed("foo")),
					Event::Text(CowStr::Borrowed("bar"))
				]
				.into_iter()
			)
			.collect::<Vec<Event<'_>>>(),
			vec![Event::Text(CowStr::Borrowed("foobar"))]
		);

		assert_eq!(
			TextMergeStream::new(
				vec![
					Event::Text(CowStr::Borrowed("foo")),
					Event::HardBreak,
					Event::Text(CowStr::Borrowed("bar"))
				]
				.into_iter()
			)
			.collect::<Vec<Event<'_>>>(),
			vec![
				Event::Text(CowStr::Borrowed("foo")),
				Event::HardBreak,
				Event::Text(CowStr::Borrowed("bar"))
			]
		);

		assert_eq!(
			TextMergeStream::new(
				vec![
					Event::Text(CowStr::Borrowed("foo")),
					Event::HardBreak,
					Event::Text(CowStr::Borrowed("bar")),
					Event::Text(CowStr::Borrowed("baz"))
				]
				.into_iter()
			)
			.collect::<Vec<Event<'_>>>(),
			vec![
				Event::Text(CowStr::Borrowed("foo")),
				Event::HardBreak,
				Event::Text(CowStr::Borrowed("barbaz"))
			]
		);

		assert_eq!(
			TextMergeStream::new(
				vec![
					Event::Text(CowStr::Borrowed("foo")),
					Event::Text(CowStr::Borrowed("bar")),
					Event::HardBreak,
					Event::Text(CowStr::Borrowed("baz"))
				]
				.into_iter()
			)
			.collect::<Vec<Event<'_>>>(),
			vec![
				Event::Text(CowStr::Borrowed("foobar")),
				Event::HardBreak,
				Event::Text(CowStr::Borrowed("baz"))
			]
		);
	}

	#[test]
	fn test_link_highlight() {
		// No event (empty stream)
		assert_eq!(
			LinkHighlightStream::new(vec![].into_iter()).collect::<Vec<Event<'_>>>(),
			vec![]
		);

		// Simple text event
		assert_eq!(
			LinkHighlightStream::new(vec![Event::Text(CowStr::Borrowed("foo"))].into_iter())
				.collect::<Vec<Event<'_>>>(),
			vec![Event::Text(CowStr::Borrowed("foo"))]
		);

		// Simple text events with hard break
		assert_eq!(
			LinkHighlightStream::new(
				vec![
					Event::Text(CowStr::Borrowed("foo")),
					Event::HardBreak,
					Event::Text(CowStr::Borrowed("bar"))
				]
				.into_iter()
			)
			.collect::<Vec<Event<'_>>>(),
			vec![
				Event::Text(CowStr::Borrowed("foo")),
				Event::HardBreak,
				Event::Text(CowStr::Borrowed("bar"))
			]
		);

		// Text containing a link
		assert_eq!(
			LinkHighlightStream::new(
				vec![Event::Text(CowStr::Borrowed("foo https://example.com bar")),].into_iter()
			)
			.collect::<Vec<Event<'_>>>(),
			vec![
				Event::Text(CowStr::Borrowed("foo ")),
				Event::Start(Tag::Link(
					LinkType::Autolink,
					CowStr::Borrowed("https://example.com"),
					CowStr::Borrowed(""),
				)),
				Event::Text(CowStr::Borrowed("https://example.com")),
				Event::End(Tag::Link(
					LinkType::Autolink,
					CowStr::Borrowed("https://example.com"),
					CowStr::Borrowed(""),
				)),
				Event::Text(CowStr::Borrowed(" bar"))
			]
		);

		// Autolink contents must be ignored as it already is a clickable link
		assert_eq!(
			LinkHighlightStream::new(
				vec![
					Event::Text(CowStr::Borrowed("foo ")),
					Event::Start(Tag::Link(
						LinkType::Autolink,
						CowStr::Borrowed("https://example.com"),
						CowStr::Borrowed(""),
					)),
					Event::Text(CowStr::Borrowed("https://example.com")),
					Event::End(Tag::Link(
						LinkType::Autolink,
						CowStr::Borrowed("https://example.com"),
						CowStr::Borrowed(""),
					)),
					Event::Text(CowStr::Borrowed(" bar"))
				]
				.into_iter()
			)
			.collect::<Vec<Event<'_>>>(),
			vec![
				Event::Text(CowStr::Borrowed("foo ")),
				Event::Start(Tag::Link(
					LinkType::Autolink,
					CowStr::Borrowed("https://example.com"),
					CowStr::Borrowed(""),
				)),
				Event::Text(CowStr::Borrowed("https://example.com")),
				Event::End(Tag::Link(
					LinkType::Autolink,
					CowStr::Borrowed("https://example.com"),
					CowStr::Borrowed(""),
				)),
				Event::Text(CowStr::Borrowed(" bar"))
			]
		);

		// Make sure that the following URLs with special characters are all recognized
		let special_urls = vec![
			"https://url.com/foo-bar",
			"https://url.com/foo_bar",
			"https://url.com/foo.bar",
			"https://url.com/foo~bar",
			"https://url.com/foo:bar",
			"https://url.com/foo/bar",
			"https://url.com/foo?bar",
			"https://url.com/foo#bar",
			"https://url.com/foo[bar",
			"https://url.com/foo]bar",
			"https://url.com/foo@bar",
			"https://url.com/foo!bar",
			"https://url.com/foo$bar",
			"https://url.com/foo&bar",
			"https://url.com/foo'bar",
			"https://url.com/foo(bar",
			"https://url.com/foo)bar",
			"https://url.com/foo*bar",
			"https://url.com/foo+bar",
			"https://url.com/foo,bar",
			"https://url.com/foo;bar",
			"https://url.com/foo=bar",
		];
		assert_eq!(
			LinkHighlightStream::new(
				special_urls
					.iter()
					.map(|text| Event::Text(CowStr::Borrowed(text)))
					.into_iter()
			)
			.collect::<Vec<Event<'_>>>(),
			special_urls
				.iter()
				.flat_map(|text| vec![
					Event::Start(Tag::Link(
						LinkType::Autolink,
						CowStr::Boxed(text.to_string().into_boxed_str()),
						CowStr::Borrowed(""),
					)),
					Event::Text(CowStr::Boxed(text.to_string().into_boxed_str())),
					Event::End(Tag::Link(
						LinkType::Autolink,
						CowStr::Boxed(text.to_string().into_boxed_str()),
						CowStr::Borrowed(""),
					)),
				]
				.into_iter())
				.collect::<Vec<Event<'_>>>()
		);

		// Make sure that the following URLs with different protocols are all recognized
		let protocol_urls = vec!["http://www.example.com", "https://www.example.com"];
		assert_eq!(
			LinkHighlightStream::new(
				protocol_urls
					.iter()
					.map(|text| Event::Text(CowStr::Borrowed(text)))
					.into_iter()
			)
			.collect::<Vec<Event<'_>>>(),
			protocol_urls
				.iter()
				.flat_map(|text| vec![
					Event::Start(Tag::Link(
						LinkType::Autolink,
						CowStr::Boxed(text.to_string().into_boxed_str()),
						CowStr::Borrowed(""),
					)),
					Event::Text(CowStr::Boxed(text.to_string().into_boxed_str())),
					Event::End(Tag::Link(
						LinkType::Autolink,
						CowStr::Boxed(text.to_string().into_boxed_str()),
						CowStr::Borrowed(""),
					)),
				]
				.into_iter())
				.collect::<Vec<Event<'_>>>()
		);
	}
}
