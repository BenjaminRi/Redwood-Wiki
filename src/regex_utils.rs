use regex::Regex;

pub trait DoPartition<'r, 't> {
	fn partition(&'r self, text: &'t str) -> Partition<'r, 't>;
}

impl<'r, 't> DoPartition<'r, 't> for Regex {
	fn partition(&'r self, text: &'t str) -> Partition<'r, 't> {
		Partition::new(self.find_iter(text), text)
	}
}

#[derive(Debug, Eq, PartialEq)]
pub enum Part<'t> {
	NoMatch(&'t str),
	Match(&'t str),
}

impl<'t> Part<'t> {
	#[allow(dead_code)]
	pub fn as_str(&self) -> &'t str {
		match &self {
			Part::NoMatch(text) => text,
			Part::Match(text) => text,
		}
	}
}

#[derive(Debug)]
enum TextMatchState<'t> {
	Init,
	Intra(regex::Match<'t>),
	Post(regex::Match<'t>),
	Done,
}

pub struct Partition<'r, 't> {
	iter: regex::Matches<'r, 't>,
	text: &'t str,
	state: TextMatchState<'t>,
}

impl<'r, 't> Partition<'r, 't> {
	pub fn new(iter: regex::Matches<'r, 't>, text: &'t str) -> Self {
		Self {
			iter,
			text,
			state: TextMatchState::Init,
		}
	}
}

impl<'r, 't> Iterator for Partition<'r, 't> {
	type Item = Part<'t>;

	fn next(&mut self) -> Option<Self::Item> {
		//println!("Next {:?}", self.state);
		match &self.state {
			TextMatchState::Init => {
				// First call of next() function
				if let Some(link) = self.iter.next() {
					let result = &self.text[..link.start()];
					self.state = TextMatchState::Intra(link);
					if result.is_empty() {
						self.next()
					} else {
						Some(Part::NoMatch(result))
					}
				} else {
					self.state = TextMatchState::Done;
					if self.text.is_empty() {
						None
					} else {
						Some(Part::NoMatch(self.text))
					}
				}
			}
			TextMatchState::Intra(link) => {
				let result = link.as_str();
				self.state = TextMatchState::Post(*link);
				if result.is_empty() {
					self.next()
				} else {
					Some(Part::Match(result))
				}
			}
			TextMatchState::Post(link) => {
				if let Some(next_link) = self.iter.next() {
					// There is another link, emit text between the links
					let result = &self.text[link.end()..next_link.start()];
					self.state = TextMatchState::Intra(next_link);
					if result.is_empty() {
						self.next()
					} else {
						Some(Part::NoMatch(result))
					}
				} else {
					// Emit the entire rest
					let result = &self.text[link.end()..];
					self.state = TextMatchState::Done;
					if result.is_empty() {
						None
					} else {
						Some(Part::NoMatch(result))
					}
				}
			}
			TextMatchState::Done => None,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_partition() {
		let regex = Regex::new(r"-*").unwrap();
		assert_eq!(regex.partition("").collect::<Vec<_>>(), vec![]);
		assert_eq!(
			regex.partition("---").collect::<Vec<_>>(),
			vec![Part::Match("---")]
		);
		assert_eq!(
			regex.partition("a---").collect::<Vec<_>>(),
			vec![Part::NoMatch("a"), Part::Match("---")]
		);
		assert_eq!(
			regex.partition("---a").collect::<Vec<_>>(),
			vec![Part::Match("---"), Part::NoMatch("a")]
		);
		assert_eq!(
			regex.partition("a---b").collect::<Vec<_>>(),
			vec![Part::NoMatch("a"), Part::Match("---"), Part::NoMatch("b")]
		);
		assert_eq!(
			regex.partition("---b--").collect::<Vec<_>>(),
			vec![Part::Match("---"), Part::NoMatch("b"), Part::Match("--")]
		);
		assert_eq!(
			regex.partition("a---b--").collect::<Vec<_>>(),
			vec![
				Part::NoMatch("a"),
				Part::Match("---"),
				Part::NoMatch("b"),
				Part::Match("--")
			]
		);
		assert_eq!(
			regex.partition("---a--b").collect::<Vec<_>>(),
			vec![
				Part::Match("---"),
				Part::NoMatch("a"),
				Part::Match("--"),
				Part::NoMatch("b")
			]
		);
		assert_eq!(
			regex.partition("a---b--c").collect::<Vec<_>>(),
			vec![
				Part::NoMatch("a"),
				Part::Match("---"),
				Part::NoMatch("b"),
				Part::Match("--"),
				Part::NoMatch("c")
			]
		);
		let regex = Regex::new(r"foo|bar").unwrap();
		assert_eq!(
			regex.partition("foobar").collect::<Vec<_>>(),
			vec![Part::Match("foo"), Part::Match("bar")]
		);
	}
}
