#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

extern crate roadrunner;
extern crate env_logger;
extern crate tokio_core;
extern crate serde_json;

use std::iter::Iterator;
use std::env;
use std::path::PathBuf;
use std::fs::File;
use std::io::Write;
use std::collections::{HashMap, HashSet};
use tokio_core::reactor::Core;
use roadrunner::RestClient;
use roadrunner::RestClientMethods;

const TMD_API_MOVIE_GENRES_URL: &str = "https://api.themoviedb.org/3/genre/movie/list";
const TMD_API_MOVIE_UPCOMING_URL: &str = "https://api.themoviedb.org/3/movie/upcoming";
const TMD_MOVIE_URL_BASE: &str = "https://www.themoviedb.org/movie";

const TMD_API_V3_ENV_KEY_NAME: &str = "TMD_API_V3";
const TMD_API_KEY_QUERY_PARAM_NAME: &str = "api_key";

// data file will be in ~/.movie_alert
const DATA_FILE_PATH: &str = ".movie_alert";

#[cfg(target_os = "macos")]
const BROWSER_OPEN_CMD: &str = "open";

#[cfg(target_os = "linux")]
const BROWSER_OPEN_CMD: &str = "xdg-open";

#[cfg(target_os = "windows")]
const BROWSER_OPEN_CMD: &str = "start";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct GenreReponse {
    genres: Vec<Genre>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Genre {
    id: u32,
    name: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct UpComingMovieResponse {
    page: u32,
    results: Vec<Movie>,
    dates: Dates,
    total_pages: u32,
    total_results: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Movie {
    poster_path: Option<String>,
    adult: bool,
    overview: String,
    release_date: String,
    genre_ids: Vec<u32>,
    id: u32,
    title: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Dates {
    maximum: String,
    minimum: String,
}


#[derive(Debug)]
enum AppError {
    APIKeyError(std::env::VarError),
    RestClientError(String, roadrunner::Error),
    GenreIdNotFoundError(String),
    HomeDirectoryError,
    SerdeJsonSerializeError(serde_json::Error),
    SerdeJsonDeserializeError(serde_json::Error),
    IOError(std::io::Error),
    EnvLogError(log::SetLoggerError),
    ReactorInitializeError(std::io::Error),
}

impl std::convert::From<std::io::Error> for AppError {
    fn from(s: std::io::Error) -> Self {
        AppError::IOError(s)
    }
}

impl AppError {
    fn report_error(self) {
        match self {
            AppError::APIKeyError(cause) => {
                error!("Error: TMD API key TMD_API_V3 is not set in env!");
                error!("    TMD API key can be obtained at https://developers.themoviedb.org/3/getting-started");
                error!("    {}", cause);
            },
            AppError::RestClientError(msg, cause) => {
                error!("{}", msg);
                error!("    {}", cause);
            },
            AppError::GenreIdNotFoundError(name) => {
                error!("Error: id cannot be found for genre name: {}", name);
            },
            AppError::HomeDirectoryError => {
                error!("Error: home directory cannot be located.")
            },
            AppError::SerdeJsonSerializeError(cause) => {
                error!("Error: cannot save to data file");
                error!("    {}", cause);
            },
            AppError::SerdeJsonDeserializeError(cause) => {
                error!("Error: cannot load from data file");
                error!("    {}", cause);
            }
            AppError::IOError(cause) => {
                error!("Error: IO error");
                error!("    {}", cause);
            },
            AppError::EnvLogError(cause) => {
                error!("Error: cannot initialize env log:");
                error!("    {}", cause);
            },
            AppError::ReactorInitializeError(cause) => {
                error!("Error: cannot initialize reactor Core:");
                error!("    {}", cause);
            },
        }
    }
}

fn main() {

    ::std::process::exit(match process() {
        Ok(_) => 0,
        Err(e) => {
            e.report_error();
            1
        },
    });
}

fn process() -> Result<(), AppError> {

    let mut core = try!(Core::new().map_err(AppError::ReactorInitializeError));

    // make it possible to see logs by:
    //          RUST_LOG="movie_alert=debug" cargo run
    //          RUST_LOG="movie_alert" cargo run
    env_logger::init()
        .map_err(AppError::EnvLogError)
        .and_then( |_| {
            // need home directory to save the data file (to keep track of
            // which movie is opened in browser).
            env::home_dir()
                .ok_or(AppError::HomeDirectoryError)
        }).and_then(|home| {
            // the movie database API key can be obtained from
            // https://developers.themoviedb.org/3/getting-started
            env::var(TMD_API_V3_ENV_KEY_NAME)
                .map_err(AppError::APIKeyError)
                .map(|key| (home, key))
        }).and_then(|(home, key)| {
            debug!("API key is found in env.");

            Ok((retrieve_genre_and_convert_to_map(&key, &mut core), key, home))
        }).and_then(move |(genre_id_to_name, key, home)| {

            let genre_animation_id: u32 = try!(get_genre_id_by_name("Animation", &genre_id_to_name));
            debug!("Animation genre id is: {}", genre_animation_id);

            let (upcoming_movies, min_date, max_date) = retrieve_all_upcoming_movies(&key, &mut core).unwrap();

            trace!("All upcoming movies: {:?}", upcoming_movies);
            debug!("Total # of upcoming movies: {}", upcoming_movies.len());

            let animation_movies = get_upcoming_movies_by_genre_id(genre_animation_id,
                    &upcoming_movies);

            println!("Upcoming animation movies (from {} to {}): {}", min_date, max_date,
                     animation_movies.len());

            let mut data_path: PathBuf = PathBuf::from(home);
            data_path.push(DATA_FILE_PATH);
            let data_path = data_path;
            debug!("Data file path is: {:?}", data_path);

            let mut opened_movie_set: HashSet<u32> = try!(load_opened_movie_set(&data_path));

            process_found_movies(&animation_movies, &genre_id_to_name, &mut opened_movie_set);

            let _ = try!(save_opened_movie_set(&opened_movie_set, &data_path));

            Ok(())
        })
}

fn load_opened_movie_set(path: &PathBuf) -> Result<HashSet<u32>, AppError> {
    if path.is_file() && path.exists() {
        let file = try!(File::open(path));

        debug!("Data file found, loading...");

        serde_json::from_reader::<_,HashSet<u32>>(file)
            .map_err(|e| AppError::SerdeJsonDeserializeError(e))
    } else {
        debug!("Data file does not exist");
        Ok(HashSet::new())
    }
}

fn save_opened_movie_set(opened_set: &HashSet<u32>, path: &PathBuf) -> Result<(), AppError> {
    let mut file = try!(File::create(path));

    let _  = try!(serde_json::to_writer(&file, opened_set)
        .map_err(|e| AppError::SerdeJsonSerializeError(e)));

    debug!("Saving data file");

    file.flush().map_err(|e| AppError::IOError(e))
}

fn process_found_movies(movies: &[&Movie], genre_map: &HashMap<u32, String>,
                        opened_movie_set: &mut HashSet<u32>) {
    for movie in movies.iter() {
        let genre_names = get_genre_name_from_ids(&movie.genre_ids, &genre_map);

        let url = TMD_MOVIE_URL_BASE.to_owned() + "/" + &movie.id.to_string();

        println!("***");
        println!("Title: {}", movie.title);
        println!("Genres: {}", genre_names);
        println!("Release date: {}", movie.release_date);
        println!("URL: {}", url);

        if opened_movie_set.contains(&movie.id) {
            println!("URL was opened")
        } else {
            let _ = std::process::Command::new(BROWSER_OPEN_CMD)
                .arg(url)
                .stdout(std::process::Stdio::inherit())
                .spawn();

            opened_movie_set.insert(movie.id);
        }
    };
}

fn get_upcoming_movies_by_genre_id(genre_id: u32, movies: &[Movie]) -> Vec<&Movie> {
    movies
        .iter()
        .filter(move |movie| {
            match movie.genre_ids.iter().find(|&&i| i == genre_id) {
                Some(_) => true,
                None => false,
            }
        }).collect()
}

fn get_genre_id_by_name(genre_name: &str, genre_map: &HashMap<u32, String>) -> Result<u32, AppError> {
    genre_map
        .iter()
        .filter(|&(_, name) | name == genre_name)
        .map(|(id, _)| id.clone())
        .last()
        .ok_or(AppError::GenreIdNotFoundError(genre_name.to_owned()))
}

fn retrieve_genre_and_convert_to_map(key: &str, core: &mut Core) -> HashMap<u32, String> {
    let genre_response = RestClient::get(TMD_API_MOVIE_GENRES_URL)
        .query_param(TMD_API_KEY_QUERY_PARAM_NAME, &key)
        .query_param("language", "en-US")
        .execute_on(core)
        .unwrap();

    trace!("Got genre response: {:?}", genre_response);

    let genre_response_typed: GenreReponse = genre_response
        .content()
        .as_typed::<GenreReponse>()
        .unwrap();
    trace!("Got typed genre response: {:?}", genre_response_typed);

    let mut genre_id_to_name: HashMap<u32, String> = HashMap::new();

    for g in genre_response_typed.genres.into_iter() {
        genre_id_to_name.insert(g.id, g.name);
    }

    let genre_id_to_name = genre_id_to_name;

    genre_id_to_name
}

fn get_genre_name_from_ids(ids: &[u32], genre_map: &HashMap<u32, String>) -> String {
    ids.iter()
        .map(|i| genre_map.get(&i) )
        .fold((String::new(), true), |(mut result, is_first), r| {
            match r {
                Some(ref s) => {
                    if !is_first {
                        result.push_str(", ");
                    }

                    result.push_str(s);
                },
                None => {},
            };

            (result, false)
        })
        .0
}

fn retrieve_all_upcoming_movies(key: &str, core: &mut Core)
                                -> Result<(Vec<Movie>, String, String), AppError> {

    retrieve_upcoming_movies_by_page(1, key, core)
        .and_then(|mut first_page_response| {
            let total_pages = first_page_response.total_pages;
            debug!("Total # of pages for upcoming movies: {}", total_pages);

            let total_movies = first_page_response.total_results;
            debug!("Total # of upcoming movies returned by page 1: {}", total_movies);

            let min_date = first_page_response.dates.minimum;
            let max_date = first_page_response.dates.maximum;

            let mut movies = Vec::new();
            movies.append(&mut first_page_response.results);

            for p in 2..(total_pages + 1) {
                let mut next_page_response = try!(retrieve_upcoming_movies_by_page(p, key, core));
                movies.append(&mut next_page_response.results);
            }

            Ok((movies, min_date, max_date))
        })
}

fn retrieve_upcoming_movies_by_page(page: u32, key: &str, core: &mut Core)
                                    -> Result<UpComingMovieResponse, AppError> {
    debug!("Getting upcoming movies, page={}", page);

    RestClient::get(TMD_API_MOVIE_UPCOMING_URL)
        .query_param(TMD_API_KEY_QUERY_PARAM_NAME, &key)
        .query_param("language", "en-US")
        .query_param("page", &page.to_string())
        .query_param("region", "US")
        .execute_on(core)
        .map_err(|e| AppError::RestClientError(
                        "Error: cannot get upcoming movies for page ".to_string() +
                            &page.to_string(), e))
        .and_then(|response| {
            trace!("Got upcoming response: {:?}", response);

            response
                .content()
                .as_typed::<UpComingMovieResponse>()
                .map_err(|e| AppError::RestClientError(
                            "Error: cannot parse upcoming movie response to json".to_string(),
                            e))
        })

}
