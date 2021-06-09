# Redwood-wiki

## Introduction

Redwood-wiki is a software to store, edit and link articles. It is similar to [MediaWiki](https://www.mediawiki.org/wiki/MediaWiki), but is more minimalist and has an emphasis on simple, robust technology and ease of use. Its intended audience is individuals or small groups of people who want to organize their knowledge.

The articles are formatted with Markdown and stored in an SQLite database. Redwood-wiki contains a web server that makes the user interface accessible through a browser. Redwood-wiki runs as a single executable, no external dependencies or programs are required.

## Design philosophy

Redwood-wiki is designed to last. This is why the implementation places a particular emphasis on robust, ubiquitous technologies and standards. Every design decision was made with great care and deliberation. The following technologies are the foundation of Redwood-wiki:

- Implemented in [Rust](https://www.rust-lang.org/)
- Stored with [SQLite](https://sqlite.org/)
- Formatted with Markdown ([CommonMark](https://commonmark.org/))
- Code highlighting with [syntect](https://github.com/trishume/syntect) using [Sublime Text syntax definitions](https://www.sublimetext.com/docs/syntax.html#include-syntax)
- Displayed with web technology ([HTTP](https://en.wikipedia.org/wiki/Hypertext_Transfer_Protocol), [HTML](https://en.wikipedia.org/wiki/HTML), [JavaScript](https://en.wikipedia.org/wiki/JavaScript) optional)

## Why should I use Redwood-wiki?

You probably shouldn't. I'm a single individual with way too little time on my hands. Some features may take years to be implemented. Go use [MediaWiki](https://www.mediawiki.org/wiki/MediaWiki).

## State of the implementation

- [x] Display articles
- [x] Create articles
- [x] Edit articles
- [ ] Search for article
- [x] Markdown highlighting in article editor
- [ ] Preview articles during editing
- [x] Code highlighting (supports Sublime Text's default open source syntax definitions)
- [x] Link to other articles (syntax not finalized and subject to change)
- [ ] Article edit history
- [ ] List all articles
- [ ] Categories
- [ ] Images
- [ ] File upload
- [ ] Configuration file (server IP, port, database location, etc.)
- [ ] Users and login
- [ ] Harden security ([CSRF](https://en.wikipedia.org/wiki/Cross-site_request_forgery), HTML injections, limit request lengths, etc.)
- [ ] Database consistency checks (dead links, etc.)

This program is in development. The basics are working and usable. However, the available code is in alpha version at best. If I change the database table layout, manual migration to newer versions may be necessary.
