use proc_macro::Span;
use proc_macro2::TokenStream;
use pyo3::type_object::PyTypeObject;
use pyo3::{PyAny, PyErr, PyResult, Python, ToPyObject};
use quote::{quote, quote_spanned};

/// Format a nice error message for a python compilation error.
pub fn compile_error_msg(py: Python, error: PyErr, tokens: TokenStream) -> TokenStream {
	let value = error.to_object(py);

	if value.is_none(py) {
		let error = format!("python: {}", error.ptype(py).name().unwrap());
		return quote!(compile_error! {#error});
	}

	if error.matches(py, pyo3::exceptions::PySyntaxError::type_object(py)) {
		let line: Option<usize> = value.getattr(py, "lineno").ok().and_then(|x| x.extract(py).ok());
		let msg: Option<String> = value.getattr(py, "msg").ok().and_then(|x| x.extract(py).ok());
		if let (Some(line), Some(msg)) = (line, msg) {
			if let Some(span) = span_for_line(tokens.clone(), line) {
				let mut error = format!("python: {}", msg);
				if msg == "cannot assign to function call" {
					error += ". LIKELY CAUSE: you cannot assign to RUST-Variables, see Context::get_global() instead";
				}
				return quote_spanned!(span.into() => compile_error!{#error});
			}
		}
	}

	if let Some(tb) = &error.ptraceback(py) {
		if let Ok((file, line)) = get_traceback_info(tb) {
			if file == Span::call_site().source_file().path().to_string_lossy() {
				if let Ok(msg) = value.as_ref(py).str() {
					if let Some(span) = span_for_line(tokens, line) {
						let error = format!("python: {}", msg);
						return quote_spanned!(span.into() => compile_error!{#error});
					}
				}
			}
		}
	}

	let error = format!("python: {}", value.as_ref(py).str().unwrap());
	quote!(compile_error! {#error})
}

fn get_traceback_info(tb: &PyAny) -> PyResult<(String, usize)> {
	let frame = tb.getattr("tb_frame")?;
	let code = frame.getattr("f_code")?;
	let file: String = code.getattr("co_filename")?.extract()?;
	let line: usize = frame.getattr("f_lineno")?.extract()?;
	Ok((file, line))
}

/// Get a span for a specific line of input from a TokenStream.
fn span_for_line(input: TokenStream, line: usize) -> Option<Span> {
	let mut spans = input
		.into_iter()
		.map(|x| x.span().unwrap())
		.skip_while(|span| span.start().line < line)
		.take_while(|span| span.start().line == line);

	let mut result = spans.next()?;
	for span in spans {
		result = match result.join(span) {
			None => return Some(result),
			Some(span) => span,
		}
	}

	Some(result)
}
