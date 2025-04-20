use crate::download::game::{Game, Kuro};

impl Kuro for Game {
    fn download(urls: Vec<String>, game_path: String) -> bool {
        false
    }

    fn patch(url: String, game_path: String, progress: impl Fn(u64, u64) + Send + 'static) -> bool {
        false
    }

    fn repair_game(res_list: String, game_path: String, is_fast: bool) -> bool {
        false
    }

    fn remove_unused_game_files(res_list: String, game_path: String) -> bool {
        false
    }
}