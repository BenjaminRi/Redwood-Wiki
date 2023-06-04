# Redwood-wiki

## Introduction

Redwood-wiki is a software to store, edit and link articles. It is similar to [MediaWiki](https://www.mediawiki.org/wiki/MediaWiki), but is more minimalist and has an emphasis on simple, robust technology and ease of use. Its intended audience is individuals or small groups of people who want to organize their knowledge.

The articles are formatted with Markdown and stored in an SQLite database. Redwood-wiki contains a web server that makes the user interface accessible through a browser. Redwood-wiki runs as a single executable, no external dependencies or programs are required.

## How to run

The wiki is fully self-contained in one single binary. To run the wiki, you need:

- The `redwood-wiki` binary, compiled for the architecture and operating system of your machine
- A configuration file with the name `wiki-config.toml` that defines things like where the wiki database should be stored and on which IP address and port the server listens

The wiki will look for `wiki-config.toml` in the current directory (`pwd`). If the file cannot be found there, it looks for the configuration in the same directory where the binary itself is located. If no configuration can be found at all or the [TOML](https://toml.io/en/) configuration file is malformed, the wiki terminates early with an error because the configuration parameters are mandatory to start the wiki.

A sample configuration file can be found in the repository at `./wiki-config.toml`. There are only a few parameters and the configuration is straightforward.

Once the wiki is running, it can be accessed at the configured IP address with a browser.

## Design philosophy

Redwood-wiki is designed to last. This is why the implementation places a particular emphasis on robust, ubiquitous technologies and standards. Every design decision was made with great care and deliberation. The following technologies are the foundation of Redwood-wiki:

- Implemented in [Rust](https://www.rust-lang.org/)
- Stored with [SQLite](https://sqlite.org/)
- Formatted with Markdown ([CommonMark](https://commonmark.org/))
- Code highlighting with [syntect](https://github.com/trishume/syntect) using [Sublime Text syntax definitions](https://www.sublimetext.com/docs/syntax.html#include-syntax)
- Displayed with web technology ([HTTP](https://en.wikipedia.org/wiki/Hypertext_Transfer_Protocol), [HTML](https://en.wikipedia.org/wiki/HTML), [JavaScript](https://en.wikipedia.org/wiki/JavaScript) optional)
- Article editing in browser with [EasyMDE](https://github.com/Ionaru/easy-markdown-editor)

## Why should I use Redwood-wiki?

You probably shouldn't. I'm a single individual with way too little time on my hands. Some features may take years to be implemented. Go use [MediaWiki](https://www.mediawiki.org/wiki/MediaWiki).

## State of the implementation

- [x] Display articles
- [x] Create articles
- [x] Edit articles
- [x] List all articles
- [x] Search for article
- [x] Markdown highlighting in article editor
- [x] Code highlighting (supports Sublime Text's default open source syntax definitions)
- [x] Link to other articles (syntax not finalized and subject to change)
- [ ] Preview article with temporary changes
- [ ] Article edit history
- [ ] Categories
- [ ] Images
- [ ] File upload
- [ ] Configuration file (server IP, port, database location, etc.)
- [ ] Users and login
- [ ] Harden security ([CSRF](https://en.wikipedia.org/wiki/Cross-site_request_forgery), HTML injections, limit request lengths, etc.)
- [ ] Database consistency checks (dead links, etc.)

This program is in development. The basics are working and usable. However, the available code is in alpha version at best. If I change the database table layout, manual migration to newer versions may be necessary.
