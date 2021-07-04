use proc_macro::{LineColumn, Span};
use proc_macro2::{Delimiter, Ident, Spacing, TokenStream, TokenTree};
use quote::quote_spanned;
use std::collections::BTreeMap;
use std::fmt::Write;

pub struct EmbedPython {
	pub python: String,
	pub variables: BTreeMap<String, Ident>,
	pub first_indent: Option<usize>,
	pub loc: LineColumn,
	pub compile_time: bool,
}

impl EmbedPython {
	pub fn new() -> Self {
		Self {
			python: String::new(),
			variables: BTreeMap::new(),
			loc: LineColumn { line: 1, column: 0 },
			first_indent: None,
			compile_time: false,
		}
	}

	fn add_whitespace(&mut self, span: Span, loc: LineColumn) -> Result<(), TokenStream> {
		#[allow(clippy::comparison_chain)]
		if loc.line > self.loc.line {
			while loc.line > self.loc.line {
				self.python.push('\n');
				self.loc.line += 1;
			}
			let first_indent = *self.first_indent.get_or_insert(loc.column);
			let indent = loc.column.checked_sub(first_indent);
			let indent = indent.ok_or_else(|| quote_spanned!(span.into() => compile_error!{"Invalid indentation"}))?;
			for _ in 0..indent {
				self.python.push(' ');
			}
			self.loc.column = loc.column;
		} else if loc.line == self.loc.line {
			while loc.column > self.loc.column {
				self.python.push(' ');
				self.loc.column += 1;
			}
		}

		Ok(())
	}

	pub fn add(&mut self, input: TokenStream) -> Result<(), TokenStream> {
		let mut tokens = input.into_iter();

		while let Some(token) = tokens.next() {
			let span = token.span().unwrap();
			self.add_whitespace(span, span.start())?;

			match &token {
				TokenTree::Group(x) => {
					let (start, end) = match x.delimiter() {
						Delimiter::Parenthesis => ("(", ")"),
						Delimiter::Brace => ("{", "}"),
						Delimiter::Bracket => ("[", "]"),
						Delimiter::None => ("", ""),
					};
					self.python.push_str(start);
					self.loc.column += start.len();
					self.add(x.stream())?;
					let mut end_loc = token.span().unwrap().end();
					end_loc.column = end_loc.column.saturating_sub(end.len());
					self.add_whitespace(span, end_loc)?;
					self.python.push_str(end);
					self.loc.column += end.len();
				}
				TokenTree::Punct(x) => {
					if !self.compile_time && x.as_char() == '\'' && x.spacing() == Spacing::Joint {
						let name = if let Some(TokenTree::Ident(name)) = tokens.next() {
							name
						} else {
							unreachable!()
						};
						let name_str = format!("_RUST_{}", name);
						self.python.push_str(&format!("((lambda:{})())", name_str));
						self.loc.column += name_str.chars().count() - 6 + 1;
						self.variables.entry(name_str).or_insert(name);
					} else if x.as_char() == '#' && x.spacing() == Spacing::Joint {
						// Convert '##' to '//', because otherwise it's
						// impossible to use the Python operators '//' and '//='.
						match tokens.next() {
							Some(TokenTree::Punct(ref p)) if p.as_char() == '#' => {
								self.python.push_str("//");
								self.loc.column += 2;
							}
							Some(TokenTree::Punct(p)) => {
								self.python.push(x.as_char());
								self.python.push(p.as_char());
								self.loc.column += 2;
							}
							_ => {
								unreachable!();
							}
						}
					} else {
						self.python.push(x.as_char());
						self.loc.column += 1;
					}
				}
				TokenTree::Ident(x) => {
					write!(&mut self.python, "{}", x).unwrap();
					self.loc = token.span().unwrap().end();
				}
				TokenTree::Literal(x) => {
					write!(&mut self.python, "{}", x).unwrap();
					self.loc = token.span().unwrap().end();
				}
			}
		}

		Ok(())
	}
}
