use warp::Filter;

//https://blog.joco.dev/posts/warp_auth_server_tutorial

#[tokio::main]
async fn main() {
    println!("Hello, world!");
	let register = warp::path::end().and(warp::get()).map(|| warp::reply::html(r#"
<!DOCTYPE html>
<html>
<body>

<h2>Redwood Wiki</h2>

<p>Welcome to Redwood Wiki!</p>

<p>Articles:</p>

<p>Users:</p>

</body>
</html>	
	"#));
    let routes = register;
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}
