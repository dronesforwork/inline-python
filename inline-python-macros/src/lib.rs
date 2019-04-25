#![recursion_limit = "128"]
#![feature(proc_macro_span)]

extern crate proc_macro;

use proc_macro::TokenStream as TokenStream1;
use proc_macro2::TokenStream;
use quote::quote;
use std::ptr::NonNull;
use std::os::raw::c_char;
use syn::{
	parse_macro_input,
	parse::{Parse, ParseStream},
};

use pyo3::{ffi, AsPyPointer, PyErr, PyObject, Python};

mod embed_python;
use embed_python::EmbedPython;

mod meta;
use self::meta::{Meta, NameValue};

#[proc_macro]
pub fn python(input: TokenStream1) -> TokenStream1 {
	let mut filename = input.clone().into_iter().next().map_or_else(
		|| String::from("<unknown>"),
		|t| t.span().source_file().path().to_string_lossy().into_owned(),
	);

	let args = parse_macro_input!(input as Args);

	let mut x = EmbedPython::new();
	x.add(args.code);

	let EmbedPython {
		mut python, variables, ..
	} = x;

	python.push('\0');
	filename.push('\0');

	let compiled = unsafe {
		let gil = Python::acquire_gil();
		let py  = gil.python();

		let compiled_code = match NonNull::new(ffi::Py_CompileString(as_c_str(&python), as_c_str(&filename), ffi::Py_file_input)) {
			None => panic!("{}", compile_error_msg(py)),
			Some(x) => PyObject::from_owned_ptr(py, x.as_ptr()),
		};

		python_marshal_object_to_bytes(py, &compiled_code).expect("marshalling compiled python code failed")
	};

	let compiled = syn::LitByteStr::new(&compiled, proc_macro2::Span::call_site());

	let make_context = match args.context {
		Some(context) => quote! {
			let _context : &::inline_python::Context = #context;
		},
		None => quote! {
			let _context = &::inline_python::Context::new_with_gil(_python_lock.python()).expect("failed to create python context");
		},
	};

	let q = quote! {
		{
			let _python_lock = ::inline_python::pyo3::Python::acquire_gil();
			#make_context
			let mut _python_variables = ::inline_python::pyo3::types::PyDict::new(_python_lock.python());
			#variables
			let r = ::inline_python::run_python_code(
				_python_lock.python(),
				_context,
				#compiled,
				Some(_python_variables)
			);
			match r {
				Ok(_) => (),
				Err(e) => {
					e.print(_python_lock.python());
					panic!("python!{...} failed to execute");
				}
			}
		}
	};

	q.into()
}

#[derive(Debug, Default)]
struct Args {
	context: Option<syn::Expr>,
	code: TokenStream,
}

fn set_once(destination: &mut Option<syn::Expr>, attribute: NameValue) -> syn::Result<()> {
	if destination.is_some() {
		Err(syn::Error::new(attribute.name.span(), "duplicate attribute"))
	} else {
		destination.replace(attribute.value);
		Ok(())
	}
}

impl Parse for Args {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let mut context = None;

		while let Some(meta) = Meta::maybe_parse(input)? {
			for attribute in meta.args.into_iter() {
				match attribute.name.to_string().as_str() {
					"context" => set_once(&mut context, attribute)?,
					_ => return Err(syn::Error::new(attribute.name.span(), "unknown attribute")),
				}
			}
		}

		Ok(Self {
			context,
			code: input.parse()?,
		})
	}
}

unsafe fn as_c_str<T: AsRef<[u8]> + ?Sized>(value: &T) -> *const c_char {
	std::ffi::CStr::from_bytes_with_nul_unchecked(value.as_ref()).as_ptr()
}

extern "C" {
	fn PyMarshal_WriteObjectToString(object: *mut ffi::PyObject, version: std::os::raw::c_int) ->  *mut ffi::PyObject;
}

/// Use built-in python marshal support to turn an object into bytes.
fn python_marshal_object_to_bytes(py: Python, object: &PyObject) -> pyo3::PyResult<Vec<u8>> {
	unsafe {
		let bytes = PyMarshal_WriteObjectToString(object.as_ptr(), 2);
		if bytes.is_null() {
			return Err(PyErr::fetch(py))
		}

		let mut buffer = std::ptr::null_mut();
		let mut size  = 0isize;
		ffi::PyBytes_AsStringAndSize(bytes, &mut buffer, &mut size);
		let result = Vec::from(std::slice::from_raw_parts(buffer as *const u8, size as usize));

		ffi::Py_DecRef(bytes);
		Ok(result)
	}
}

/// Convert a PyUnicode object to String.
unsafe fn py_unicode_string(object: *mut ffi::PyObject) -> String {
	let mut size = 0isize;
	let data = ffi::PyUnicode_AsUTF8AndSize(object, &mut size) as *const u8;
	let data = std::slice::from_raw_parts(data, size as usize);
	let data = std::str::from_utf8_unchecked(data);
	String::from(data)
}

/// Convert a python object to a string using the the python `str()` function.
fn python_str(object: &PyObject) -> String {
	unsafe {
		let string = ffi::PyObject_Str(object.as_ptr());
		let result = py_unicode_string(string);
		ffi::Py_DecRef(string);
		result
	}
}

/// Get the object of a PyErrValue, if any.
fn err_value_object(py: Python, value: pyo3::PyErrValue) -> Option<PyObject> {
	match value {
		pyo3::PyErrValue::None        => None,
		pyo3::PyErrValue::Value(x)    => Some(x),
		pyo3::PyErrValue::ToArgs(x)   => Some(x.arguments(py)),
		pyo3::PyErrValue::ToObject(x) => Some(x.to_object(py)),
	}
}

fn compile_error_msg(py: Python) -> String {
	use pyo3::type_object::PyTypeObject;
	use pyo3::AsPyRef;

	if !PyErr::occurred(py) {
		return String::from("failed to compile python code, but no detailed error is available");
	}

	let error = PyErr::fetch(py);

	if error.matches(py, pyo3::exceptions::SyntaxError::type_object()) {
		let PyErr { ptype: kind, pvalue: value, .. } = error;
		let value = match err_value_object(py, value) {
			None    => return kind.as_ref(py).name().into_owned(),
			Some(x) => x,
		};

		return match value.extract::<(String, (String, i32, i32, String))>(py) {
			Ok((msg, (file, line, col, _token))) => format!("{} at {}:{}:{}", msg, file, line, col),
			Err(_) => python_str(&value),
		};
	}

	let PyErr { ptype: kind, pvalue: value, .. } = error;
	match err_value_object(py, value) {
		None    => kind.as_ref(py).name().into_owned(),
		Some(x) => python_str(&x),
	}
}
