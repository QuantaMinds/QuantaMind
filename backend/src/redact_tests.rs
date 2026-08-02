use super::*;

#[test]
fn masks_the_real_home_dir_to_tilde() {
    if let Some(home) = dirs::home_dir() {
        let p = format!("{}/.quantamind/gguf/phi.gguf", home.to_string_lossy());
        let red = redact_path(&p);
        assert!(red.starts_with("~/.quantamind"), "got {red}");
        assert!(!red.contains(home.to_string_lossy().as_ref()), "home dir survived: {red}");
    }
}

#[test]
fn masks_generic_user_segment_for_another_user() {
    assert_eq!(
        redact_path("/Users/alice/models/x.gguf"),
        "/Users/<user>/models/x.gguf"
    );
    assert_eq!(redact_path("/home/bob/.cache/hf"), "/home/<user>/.cache/hf");
    assert_eq!(
        redact_path("C:\\Users\\carol\\AppData\\x"),
        "C:\\Users\\<user>\\AppData\\x"
    );
}

#[test]
fn leaves_non_home_paths_untouched() {
    assert_eq!(redact_path("/usr/local/bin/the server"), "/usr/local/bin/the server");
    assert_eq!(redact_path("qwen3:8b"), "qwen3:8b");
    assert_eq!(redact_path("meta-llama/Llama-3-8B"), "meta-llama/Llama-3-8B");
}

#[test]
fn handles_multiple_occurrences_and_no_trailing_segment() {
    assert_eq!(
        redact_path("copy /Users/alice/a to /Users/alice/b"),
        "copy /Users/<user>/a to /Users/<user>/b"
    );
    // marker with nothing after it → nothing to mask, no panic.
    assert_eq!(redact_path("/Users/"), "/Users/");
}
