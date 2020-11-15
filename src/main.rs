use warp::Filter;

use chrono;

use rusqlite::{params, Connection, Result};

use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug)]
struct Person {
    id: i32,
    name: String,
    data: Option<Vec<u8>>,
}

//https://blog.joco.dev/posts/warp_auth_server_tutorial

struct Database {
	conn: rusqlite::Connection,
}

#[tokio::main]
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
    .apply().unwrap();
	
	
    log::info!("Starting Redwood-Wiki!");
	
	//SQLITE TEST
	
	let conn = Connection::open("test.sqlite").unwrap();
	
	let db = Arc::new(Mutex::new(Database {conn}));
    let db = warp::any().map(move || db.clone());
	
	/*conn.execute(
		//"CREATE TABLE person (
        "CREATE TABLE IF NOT EXISTS person (
                  id              INTEGER PRIMARY KEY,
                  name            TEXT NOT NULL,
                  data            BLOB
                  )",
        params![],
    ).unwrap();
    let me = Person {
        id: 0,
        name: "Steven".to_string(),
        data: None,
    };
    conn.execute(
        "INSERT INTO person (name, data) VALUES (?1, ?2)",
        params![me.name, me.data],
    ).unwrap();

    let mut stmt = conn.prepare("SELECT id, name, data FROM person").unwrap();
    let person_iter = stmt.query_map(params![], |row| {
        Ok(Person {
            id: row.get(0)?,
            name: row.get(1)?,
            data: row.get(2)?,
        })
    }).unwrap();

    for person in person_iter {
        println!("Found person {:?}", person.unwrap());
    }*/
	
	//END SQLITE TEST
	
	let index_path = warp::path::end();
    let routes = index_path.and(db).and_then(index_page);
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn index_page(db: Arc<Mutex<Database>>) -> Result<impl warp::Reply, warp::Rejection> {
    let mut db = db.lock().await;
	/*.and(warp::get()).map(|| warp::reply::html(r#"
<!DOCTYPE html>
<html>
<body>

<h2>Redwood Wiki</h2>

<p>Welcome to Redwood Wiki!</p>

<p>Articles:</p>

<p>Users:</p>

</body>
</html>	
	"#))*/
    Ok("hello".to_string())
}
