/*!
A derive proc macro that allow a struct to be read from a database using the mssql_client crate.

# Example
```
use mssql_client::Connection;
use mssql_client_derive::Sql;
use futures::future::Future;
use tokio::executor::current_thread::block_on_all;

#[derive(Sql)]
struct MyRow {
    id: i32,
    name: String,

    #[sql(default)]
    others: Vec<usize>,
}

fn main() {
    // `MyRow` struct has a method implemented that gives the sql fields
    assert_eq!("[Id],[Name]", MyRow::sql_fields_str());

    let sql = format!(
        r#"
        SELECT {}
        FROM
        (
            SELECT CAST(1 as int) id, 'foo' name
        ) t
        "#,
        MyRow::sql_fields_str()
    );

    // load the struct from the database
    let conn_str = "server=tcp:localhost\\SQL2017;database=master;integratedsecurity=sspi;trustservercertificate=true";

    let fut = Connection::connect(conn_str)
        // `FromRow` trait is implemented on `MyRow` struct
        .and_then(|conn| conn.query::<MyRow, _, _>(sql, ()));

    let (_, rows) = block_on_all(fut).unwrap();

    assert_eq!(1, rows[0].id);
    assert_eq!("foo", &rows[0].name);
}
!*/

extern crate proc_macro;

use inflector::Inflector;
use itertools::Itertools;
use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream, Result};
use syn::{
    parenthesized, parse, parse_macro_input, Attribute, Data, DeriveInput, Error, Expr, Field,
    Fields, Ident, LitInt, LitStr, Token,
};

#[proc_macro_derive(Sql, attributes(sql))]
pub fn sql(input: TokenStream) -> TokenStream {
    let derive_input = parse_macro_input!(input as DeriveInput);

    let s = match Struct::from_derive_input(derive_input) {
        Ok(s) => s,
        Err(e) => {
            return e.to_compile_error().into();
        }
    };

    let sql_fields = s.fields.iter().filter_map(|f| f.sql()).join(",");
    let name = s.name.clone();

    let row_gets = s
        .fields
        .iter()
        .filter_map(|f| match f {
            SqlField::Expr { .. } => None,
            SqlField::SqlNamed { ident, .. } | SqlField::SqlUnnamed(ident) => Some(ident),
        })
        .enumerate()
        .map(|(index, ident)| {
            let error = LitStr::new("Read `{}` failed; {}", ident.span());
            let field = LitStr::new(&ident.to_string(), ident.span());
            let index = LitInt::new(&index.to_string(), ident.span());
            quote!(#ident: row.get(#index).map_err(|e| failure::format_err!(#error, #field, e))?)
        });

    let expr_gets = s.fields.iter().filter_map(|f| match f {
        SqlField::Expr { ident, expr } => Some(quote!(#ident: #expr)),
        SqlField::SqlNamed { .. } | SqlField::SqlUnnamed(_) => None,
    });

    quote!(
        impl #name {
            pub(crate) fn sql_fields_str() -> &'static str {
                #sql_fields
            }
        }

        impl mssql_client::FromRow for #name {
            fn from_row(row: &mssql_client::Row) -> Result<Self, failure::Error> {
                Ok(Self {
                    #(#row_gets,)*
                    #(#expr_gets,)*
                })
            }
        }
    )
    .into()
}

struct Struct {
    fields: Vec<SqlField>,
    name: Ident,
}

impl Struct {
    fn from_derive_input(input: DeriveInput) -> Result<Self> {
        let name = input.ident;

        let data_struct = match input.data {
            Data::Struct(s) => s,
            _ => {
                return Err(Error::new(
                    name.span(),
                    "Only struct are supported by sql derive.",
                ))
            }
        };

        let mut fields = Vec::new();

        match data_struct.fields {
            Fields::Named(f) => {
                for f in &f.named {
                    fields.push(SqlField::new_from_field(f)?);
                }
            }
            _ => {
                return Err(Error::new(
                    name.span(),
                    "Only struct with named fields are supported by sql derive.",
                ))
            }
        };

        Ok(Struct { fields, name })
    }
}

enum SqlField {
    Expr { ident: Ident, expr: Expr },
    SqlNamed { ident: Ident, name: LitStr },
    SqlUnnamed(Ident),
}

impl SqlField {
    fn new_from_field(f: &Field) -> Result<Self> {
        let ident = f
            .ident
            .clone()
            .expect("Sql struct field must be named.")
            .clone();

        match f.attrs.iter().filter_map(SqlAttr::try_new).next() {
            Some(Ok(SqlAttr::Expr(expr))) => Ok(SqlField::Expr { ident, expr }),
            Some(Ok(SqlAttr::Default)) => Ok(SqlField::Expr {
                ident,
                expr: parse(quote!(Default::default()).into())?,
            }),
            Some(Ok(SqlAttr::Name(name))) => Ok(SqlField::SqlNamed { ident, name }),
            Some(Err(e)) => Err(e),
            None => Ok(SqlField::SqlUnnamed(ident)),
        }
    }

    fn sql(&self) -> Option<String> {
        match self {
            SqlField::Expr { .. } => None,
            SqlField::SqlNamed { name, .. } => Some(name.value()),
            SqlField::SqlUnnamed(ident) => {
                Some(format!("[{}]", ident.to_string().to_pascal_case()))
            }
        }
    }
}

enum SqlAttr {
    Default,
    Expr(Expr),
    Name(LitStr),
}

impl SqlAttr {
    fn try_new(a: &Attribute) -> Option<Result<Self>> {
        let name = a
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .join("::");
        match name.as_str() {
            "sql" | "sql_derive::sql" => Some(Self::parse_tts(a.tokens.clone().into())),
            _ => None,
        }
    }

    fn parse_tts(input: TokenStream) -> Result<Self> {
        parse(input)
    }
}

impl Parse for SqlAttr {
    fn parse(input: ParseStream) -> Result<SqlAttr> {
        let content;
        let _ = parenthesized!(content in input);
        let ident: Ident = content.parse()?;

        let out = match ident.to_string().as_str() {
            "default" => SqlAttr::Default,
            "expr" => {
                let _: Token![=] = content.parse()?;
                SqlAttr::Expr(content.parse()?)
            }
            "name" => {
                let _: Token![=] = content.parse()?;
                SqlAttr::Name(content.parse()?)
            }
            _ => {
                return Err(Error::new(
                    ident.span(),
                    "Expect `default`, `expr`, or `name`",
                ))
            }
        };

        Ok(out)
    }
}
