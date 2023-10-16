## HTML cleanliness

- Clean up: `<p>` tags do not allow `<form>` and `<ul>`/`<li>` tags inside (see https://stackoverflow.com/questions/9852312/list-of-html5-elements-that-can-be-nested-inside-p-element for allowed tags)

## Security

- Limit article title length to a sensible number
- Limit article length?
- Limit request length (warp::body::content_length_limit)
- Sanitize text to prevent HTML injections (e.g. in article title, text)
- Prevent CSRF
