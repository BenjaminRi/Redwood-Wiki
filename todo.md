


## Security

- Limit article title length to a sensible number
- Limit article length?
- Limit request length (warp::body::content_length_limit)
- Sanitize text to prevent HTML injections (e.g. in article title, text)
- Prevent CSRF

## Misc

- Activate markdown tables

## URL scheme

- + and - signs work (prevent this)
- negative indices for articles don't give http method not allowed
