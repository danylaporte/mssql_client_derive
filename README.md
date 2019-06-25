[![Build Status](https://travis-ci.org/danylaporte/mssql_client_derive.svg?branch=master)](https://travis-ci.org/danylaporte/mssql_client_derive)

A derive proc macro that allow a struct to be read from a database using the mssql_client crate.

## Documentation
[API Documentation](https://danylaporte.github.io/mssql_client_derive/mssql_client_derive)

## Example

```rust
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
```

## License

Dual-licensed to be compatible with the Rust project.

Licensed under the Apache License, Version 2.0
[http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0) or the MIT license
[http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT), at your
option. This file may not be copied, modified, or distributed
except according to those terms.