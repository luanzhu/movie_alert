# Movie Alert

This is a simple toy project to hit [themoviedb.org api](https://developers.themoviedb.org/3/getting-started)
to alert (by opening movie URLs in browser) about upcoming animation movies.

I tried to use this project to test the [RoadRunner rest client](https://github.com/luanzhu/roadrunner).
However, only GET calls are used.  Looks like I picked a wrong project to
test RoadRunner. :)

# TMD API Key
This program expects themoviedb.org API key in the environment.

```bash
export TMD_API_V3=your_fandango_api_key

```

To get a key, please visit [The Movie Database API Getting Started](https://developers.themoviedb.org/3/getting-started).

# Run


```bash
cargo run

```

Or, to see more logs:

```bash
RUST_LOG="movie_alert=debug" cargo run
RUST_LOG="movie_alert" cargo run
```
