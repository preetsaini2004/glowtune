use std::{
    fs,
    sync::mpsc,
    time::{Duration, Instant},
};

use tempfile::{TempDir, tempdir};
use themes::*;

fn parse(text: &str) -> (Theme, Vec<Warning>) {
    Theme::from_toml_str(text).expect("valid test TOML")
}

fn store() -> (TempDir, ThemeStore) {
    let dir = tempdir().expect("temporary directory");
    let store = ThemeStore::new(dir.path().join("user")).expect("create store");
    (dir, store)
}

fn named(name: &str) -> Theme {
    let mut theme = Theme::default();
    theme.meta.name = name.into();
    theme
}

#[test]
fn color_formats_and_case() {
    for (input, output) in [
        (" #AbC ", "#aabbcc"),
        ("#ABCDEF", "#abcdef"),
        ("#AbCdEf12", "#abcdef12"),
        ("#123456ff", "#123456"),
    ] {
        assert_eq!(Color::parse(input).expect("color").to_hex(), output);
    }
}

#[test]
fn color_rejects_bad_input_without_panics() {
    for input in [
        "",
        "fff",
        "#12",
        "#1234",
        "#12345",
        "#1234567",
        "#ggg",
        "#あ",
        "#éabcd",
        "#123456789",
        "#12 456",
    ] {
        assert_eq!(Color::parse(input), None, "{input}");
    }
}

#[test]
fn color_serde_is_hex() {
    let color = Color {
        r: 1,
        g: 2,
        b: 3,
        a: 4,
    };
    let value = toml::Value::try_from(color).expect("serialize color");
    assert_eq!(value.as_str(), Some("#01020304"));
    assert_eq!(value.try_into::<Color>().expect("deserialize color"), color);
    assert!(
        toml::Value::String("red".into())
            .try_into::<Color>()
            .is_err()
    );
}

#[test]
fn default_is_midnight_glass() {
    let (_, text) = bundled()[0];
    let (theme, warnings) = parse(text);
    assert_eq!(theme, Theme::default());
    assert!(warnings.is_empty());
}

#[test]
fn every_bundle_is_valid() {
    assert_eq!(bundled().len(), 3);
    for (id, text) in bundled() {
        assert!(text.starts_with('#'));
        let (theme, warnings) = parse(text);
        assert!(warnings.is_empty(), "{id}: {warnings:?}");
        assert!(!theme.meta.name.is_empty());
    }
}

#[test]
fn bundled_styles_match_spec() {
    let (_dir, store) = store();
    let aurora = store.load("aurora").expect("aurora").0;
    assert_eq!(aurora.colors.mode, ColorMode::Gradient);
    assert_eq!(aurora.colors.glow.len(), 3);
    assert_eq!(aurora.glass.blur, 32);
    assert_eq!(aurora.glow.drift_speed, 1.4);
    let ember = store.load("ember").expect("ember").0;
    assert_eq!(ember.colors.mode, ColorMode::Static);
    assert_eq!(ember.glow.corners, Corners::Square);
    assert_eq!(ember.lyrics.style, LyricsStyle::Karaoke);
}

#[test]
fn glow_effects_roundtrip_and_repair_invalid_values() {
    for (name, effect) in [
        ("ambient", GlowEffect::Ambient),
        ("still", GlowEffect::Still),
        ("pulse", GlowEffect::Pulse),
        ("orbit", GlowEffect::Orbit),
        ("distortion", GlowEffect::Distortion),
        ("lava-lamp", GlowEffect::LavaLamp),
    ] {
        let (theme, warnings) = parse(&format!("[glow]\neffect = '{name}'\neffect_speed = 2.5"));
        assert!(warnings.is_empty());
        assert_eq!(theme.glow.effect, effect);
        assert_eq!(theme.glow.effect_speed, 2.5);
        let (restored, warnings) = parse(&theme.to_toml_string());
        assert!(warnings.is_empty());
        assert_eq!(restored, theme);
    }
    let (old, _) = parse("[glow]\nbeat_reactivity = 0.9");
    assert_eq!(old.glow.effect, GlowEffect::Ambient);
    assert_eq!(old.glow.effect_speed, 1.0);
    for (speed, expected) in [("-1", 0.0), ("9", 3.0), ("nan", 1.0)] {
        let (theme, warnings) = parse(&format!(
            "[glow]\neffect = 'unknown'\neffect_speed = {speed}"
        ));
        assert_eq!(theme.glow.effect, GlowEffect::Ambient);
        assert_eq!(theme.glow.effect_speed, expected);
        assert_eq!(warnings.len(), 2);
    }
}

#[test]
fn particles_roundtrip_defaults_and_numeric_repair() {
    let (old, warnings) = parse("[glow]\neffect = 'lava-lamp'");
    assert!(warnings.is_empty());
    assert_eq!(old.glow.particles, ParticleStyle::Off);
    assert_eq!(old.glow.effect_strength, 1.0);
    assert!(!old.glow.reverse_motion);
    for (name, style) in [
        ("off", ParticleStyle::Off),
        ("dots", ParticleStyle::Dots),
        ("stars", ParticleStyle::Stars),
        ("dust", ParticleStyle::Dust),
    ] {
        let (theme, warnings) = parse(&format!(
            "[glow]\nparticles = '{name}'\nparticle_density = 0.8\nparticle_size = 2.5\nparticle_speed = 1.4\nparticle_sparkle = 0.2\neffect_strength = 0.6\nreverse_motion = true"
        ));
        assert!(warnings.is_empty());
        assert_eq!(theme.glow.particles, style);
        assert!(theme.glow.reverse_motion);
        let (restored, warnings) = parse(&theme.to_toml_string());
        assert!(warnings.is_empty());
        assert_eq!(restored, theme);
    }
    let (repaired, warnings) = parse(
        "[glow]\nparticles = 'unknown'\nparticle_density = 8\nparticle_size = -1\nparticle_speed = nan\nparticle_sparkle = inf\neffect_strength = -2\nreverse_motion = 'yes'",
    );
    assert_eq!(warnings.len(), 7);
    assert_eq!(repaired.glow.particles, ParticleStyle::Off);
    assert_eq!(repaired.glow.particle_density, 1.0);
    assert_eq!(repaired.glow.particle_size, 0.5);
    assert_eq!(repaired.glow.particle_speed, 1.0);
    assert_eq!(repaired.glow.particle_sparkle, 0.5);
    assert_eq!(repaired.glow.effect_strength, 0.0);
    assert!(!repaired.glow.reverse_motion);
}

#[test]
fn missing_fields_use_defaults_with_identity_fallback() {
    let (theme, warnings) = parse("");
    let mut expected = Theme::default();
    expected.meta.name = "Untitled theme".into();
    expected.meta.author.clear();
    assert_eq!(theme, expected);
    assert!(warnings.is_empty());
}

#[test]
fn syntax_error_includes_line_and_column() {
    let error = Theme::from_toml_str("[glow]\nthickness = [").expect_err("syntax error");
    assert!(matches!(error, Error::TomlSyntax(_)));
    assert!(error.to_string().contains("line"), "{error}");
    assert!(error.to_string().contains("column"), "{error}");
}

#[test]
fn bad_section_recovers_without_losing_other_sections() {
    let (theme, warnings) = parse("glass = 4\n[glow]\nthickness = 100");
    assert_eq!(theme.glass, Glass::default());
    assert_eq!(theme.glow.thickness, 100);
    assert_eq!(warnings[0].path, "glass");
}

#[test]
fn unknown_sections_and_fields_warn() {
    let (_, warnings) = parse("oops = 1\n[glow]\ntyop = true\n[mystery]\nvalue = 3");
    assert_eq!(warnings.len(), 3);
    for path in ["oops", "glow.tyop", "mystery"] {
        assert!(
            warnings
                .iter()
                .any(|w| w.path == path && w.message == "unknown key")
        );
    }
}

#[test]
fn wrong_types_fall_back_per_field() {
    let (theme, warnings) = parse(
        "[meta]\nname = 7\nauthor = []\n[glass]\nblur = 'large'\nopacity = false\n[glow]\nenabled = 1\nthickness = 80\n[lyrics]\nshow_on_desktop = 'true'",
    );
    assert_eq!(theme.meta.name, "Untitled theme");
    assert_eq!(theme.meta.author, "");
    assert_eq!(theme.glass, Glass::default());
    assert!(theme.glow.enabled);
    assert!(theme.lyrics.show_on_desktop);
    assert_eq!(theme.glow.thickness, 80);
    assert_eq!(warnings.len(), 6);
}

#[test]
fn integer_limits_clamp_including_negative_and_huge() {
    let (theme, warnings) = parse(
        "[glass]\nblur = -1\nradius = 9223372036854775807\n[font]\nlyrics_size = 2\n[glow]\nthickness = 900",
    );
    assert_eq!(
        (
            theme.glass.blur,
            theme.glass.radius,
            theme.font.lyrics_size,
            theme.glow.thickness
        ),
        (0, 40, 16, 120)
    );
    assert_eq!(warnings.len(), 4);
}

#[test]
fn integer_fields_reject_fractional_values() {
    let (theme, warnings) = parse("[glass]\nblur = 12.5");
    assert_eq!(theme.glass.blur, 24);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn floats_accept_integers() {
    let (theme, warnings) =
        parse("[glass]\nopacity = 1\n[glow]\nintensity = 1\nbeat_reactivity = 0\ndrift_speed = 10");
    assert_eq!(theme.glass.opacity, 1.0);
    assert_eq!(theme.glow.intensity, 1.0);
    assert_eq!(theme.glow.beat_reactivity, 0.0);
    assert_eq!(theme.glow.drift_speed, 10.0);
    assert!(warnings.is_empty());
}

#[test]
fn float_limits_clamp() {
    let (theme, warnings) = parse(
        "[glass]\nopacity = -0.4\n[glow]\nintensity = 9.0\nbeat_reactivity = 2.0\ndrift_speed = -1",
    );
    assert_eq!(theme.glass.opacity, 0.0);
    assert_eq!(theme.glow.intensity, 1.5);
    assert_eq!(theme.glow.beat_reactivity, 1.0);
    assert_eq!(theme.glow.drift_speed, 0.0);
    assert_eq!(warnings.len(), 4);
}

#[test]
fn nonfinite_numbers_fall_back() {
    let (theme, warnings) = parse(
        "[glass]\nopacity = nan\n[glow]\nintensity = inf\nbeat_reactivity = -inf\ndrift_speed = +nan",
    );
    assert_eq!(theme.glass.opacity, 0.55);
    assert_eq!(theme.glow, Glow::default());
    assert_eq!(warnings.len(), 4);
}

#[test]
fn invalid_enums_fall_back() {
    let (theme, warnings) = parse(
        "[colors]\nmode = 'Adaptive'\n[glow]\ncorners = 'triangle'\n[lyrics]\nstyle = 'fly'\nposition = 'left'",
    );
    assert_eq!(theme.colors.mode, ColorMode::Adaptive);
    assert_eq!(theme.glow.corners, Corners::Rounded);
    assert_eq!(theme.lyrics, Lyrics::default());
    assert_eq!(warnings.len(), 4);
}

#[test]
fn invalid_hex_is_local_to_its_field() {
    let (theme, warnings) = parse("[colors]\nbackground = 'black'\naccent = '#123'\ntext = 12");
    assert_eq!(theme.colors.background, Colors::default().background);
    assert_eq!(theme.colors.accent.to_hex(), "#112233");
    assert_eq!(theme.colors.text, Colors::default().text);
    assert_eq!(warnings.len(), 2);
}

#[test]
fn glow_list_bad_entries_keep_good_entries() {
    let (theme, warnings) = parse("[colors]\nglow = ['#123', false, '#456', 'bad']");
    assert_eq!(
        theme
            .colors
            .glow
            .iter()
            .map(Color::to_hex)
            .collect::<Vec<_>>(),
        ["#112233", "#5ca0ff", "#445566", "#5ca0ff"]
    );
    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[0].path, "colors.glow[1]");
}

#[test]
fn glow_list_empty_or_wrong_type_defaults() {
    for value in ["[]", "42", "'#fff'"] {
        let (theme, warnings) = parse(&format!("[colors]\nglow = {value}"));
        assert_eq!(theme.colors.glow, Colors::default().glow);
        assert_eq!(warnings.len(), 1);
    }
}

#[test]
fn glow_list_truncates() {
    let (theme, warnings) =
        parse("[colors]\nglow = ['#111', '#222', '#333', '#444', '#555', '#666']");
    assert_eq!(theme.colors.glow.len(), 5);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn font_names_validate_characters_not_bytes() {
    let input = format!(
        "[font]\nfamily = '{}'\nlyrics_family = '{}'",
        "界".repeat(64),
        "x".repeat(65)
    );
    let (theme, warnings) = parse(&input);
    assert_eq!(theme.font.family.chars().count(), 64);
    assert_eq!(theme.font.lyrics_family, Font::default().lyrics_family);
    assert_eq!(warnings.len(), 1);
    let (theme, warnings) = parse("[font]\nfamily = '   '\nlyrics_family = ''");
    assert_eq!(theme.font, Font::default());
    assert_eq!(warnings.len(), 2);
}

#[test]
fn meta_name_and_version_validate() {
    for value in ["'one'", "1.0", "true", "[]"] {
        let (theme, warnings) = parse(&format!("[meta]\nname = ''\nversion = {value}"));
        assert_eq!(theme.meta.name, "Untitled theme");
        assert_eq!(theme.meta.version, 1);
        assert_eq!(warnings.len(), 2);
    }
}

#[test]
fn schema_version_preserves_toml_integers() {
    for version in [i64::MIN, -1, 0, 2, i64::MAX] {
        let (theme, warnings) = parse(&format!("[meta]\nversion = {version}"));
        assert_eq!(theme.meta.version, version);
        assert!(warnings.is_empty());
        assert_eq!(parse(&theme.to_toml_string()).0, theme);
    }
}

#[test]
fn monitors_all_primary_and_named_arrays() {
    for (value, expected) in [
        ("'all'", Monitors::All),
        ("'primary'", Monitors::Primary),
        (
            "['Display 1', 'Display 2']",
            Monitors::Only(vec!["Display 1".into(), "Display 2".into()]),
        ),
        ("[]", Monitors::Only(vec![])),
    ] {
        let (theme, warnings) = parse(&format!("[glow]\nmonitors = {value}"));
        assert_eq!(theme.glow.monitors, expected);
        assert!(warnings.is_empty());
    }
}

#[test]
fn invalid_monitor_values_default() {
    for value in ["'secondary'", "12", "['Display', 3]", "{}"] {
        let (theme, warnings) = parse(&format!("[glow]\nmonitors = {value}"));
        assert_eq!(theme.glow.monitors, Monitors::All);
        assert_eq!(warnings.len(), 1);
    }
}

#[test]
fn roundtrip_bundles_and_all_enum_variants() {
    let mut themes: Vec<_> = bundled().iter().map(|(_, text)| parse(text).0).collect();
    let mut theme = Theme::default();
    theme.glow.monitors = Monitors::Only(vec!["A \"wide\" monitor".into(), "界".into()]);
    theme.lyrics.position = LyricsPosition::TopCenter;
    theme.lyrics.style = LyricsStyle::Static;
    themes.push(theme.clone());
    theme.glow.monitors = Monitors::Primary;
    theme.lyrics.position = LyricsPosition::Center;
    themes.push(theme);
    for theme in themes {
        let text = theme.to_toml_string();
        let sections: Vec<_> = text.lines().filter(|line| line.starts_with('[')).collect();
        assert_eq!(
            sections,
            [
                "[meta]", "[colors]", "[glass]", "[font]", "[glow]", "[lyrics]"
            ]
        );
        let (loaded, warnings) = parse(&text);
        assert_eq!(theme, loaded);
        assert!(warnings.is_empty(), "{warnings:?}");
        let strict: Theme = toml::from_str(&text).expect("serde roundtrip");
        assert_eq!(strict, loaded);
    }
}

#[test]
fn sanitized_repairs_in_memory_values_and_is_idempotent() {
    let mut theme = named(" ");
    theme.meta.version = 99;
    theme.font.family.clear();
    theme.font.lyrics_family = "x".repeat(65);
    theme.font.lyrics_size = 0;
    theme.glass.blur = 999;
    theme.glass.radius = 999;
    theme.glass.opacity = f64::NAN;
    theme.glow.thickness = 0;
    theme.glow.intensity = -5.0;
    theme.glow.beat_reactivity = f64::INFINITY;
    theme.glow.drift_speed = 99.0;
    theme.colors.glow.clear();
    let (theme, warnings) = theme.sanitized();
    assert_eq!(warnings.len(), 12);
    assert_eq!(theme.meta.name, "Untitled theme");
    assert_eq!(theme.glass.blur, 64);
    assert_eq!(theme.glow.intensity, 0.2);
    assert_eq!(theme.glow.drift_speed, 10.0);
    assert!(theme.clone().sanitized().1.is_empty());
    assert_eq!(parse(&theme.to_toml_string()).0, theme);
}

#[test]
fn warning_display_includes_path_and_message() {
    let warning = Warning {
        path: "glass.blur".into(),
        message: "clamped".into(),
    };
    assert_eq!(warning.to_string(), "glass.blur: clamped");
}

#[test]
fn store_creates_directory_and_lists_bundles() {
    let (dir, store) = store();
    assert!(dir.path().join("user").is_dir());
    let list = store.list();
    assert_eq!(
        list.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        ["midnight-glass", "aurora", "ember"]
    );
    assert!(list.iter().all(|i| i.bundled));
}

#[test]
fn store_save_load_and_overwrite() {
    let (dir, store) = store();
    let mut theme = named("My Theme");
    let id = store.save(&theme).expect("save");
    assert_eq!(id, "my-theme");
    assert_eq!(store.load(&id).expect("load"), (theme.clone(), vec![]));
    theme.glow.enabled = false;
    assert_eq!(store.save(&theme).expect("overwrite"), id);
    assert_eq!(store.load(&id).expect("reload").0, theme);
    assert_eq!(
        fs::read_dir(dir.path().join("user"))
            .expect("entries")
            .count(),
        1
    );
}

#[test]
fn store_save_sanitizes_without_changing_original() {
    let (_dir, store) = store();
    let mut theme = named("Bad range");
    theme.glow.thickness = 999;
    let id = store.save(&theme).expect("save");
    assert_eq!(theme.glow.thickness, 999);
    assert_eq!(store.load(&id).expect("load").0.glow.thickness, 120);
}

#[test]
fn store_slug_collapses_limits_and_falls_back() {
    let (_dir, store) = store();
    for (name, id) in [
        (" -- Hello___WORLD!! ", "hello-world".to_owned()),
        ("界🎵", "theme".to_owned()),
        ("", "theme".to_owned()),
        (&"A".repeat(80), "a".repeat(48)),
    ] {
        assert_eq!(store.save(&named(name)).expect("save slug"), id);
    }
}

#[test]
fn store_list_sorts_users_and_skips_unreadable_or_invalid() {
    let (dir, store) = store();
    store.save(&named("Zulu")).expect("save");
    store.save(&named("Alpha")).expect("save");
    let user = dir.path().join("user");
    fs::write(user.join("invalid.toml"), "[broken").expect("write");
    fs::write(user.join("bytes.toml"), [0xff]).expect("write");
    fs::write(user.join("ignore.txt"), "").expect("write");
    fs::write(user.join("UNSAFE.toml"), "").expect("write");
    fs::create_dir(user.join("directory.toml")).expect("directory");
    let list = store.list();
    assert_eq!(list.len(), 5);
    assert_eq!(list[3].name, "Alpha");
    assert_eq!(list[4].name, "Zulu");
    assert!(!list[3].bundled);
}

#[test]
fn user_override_replaces_bundle_and_delete_reveals_original() {
    let (_dir, store) = store();
    let mut theme = named("Aurora");
    theme.meta.author = "User".into();
    store.save(&theme).expect("override");
    assert_eq!(store.load("aurora").expect("load").0, theme);
    let list = store.list();
    assert_eq!(list.len(), 3);
    assert_eq!(list[2].id, "aurora");
    assert!(!list[2].bundled);
    store.delete("aurora").expect("delete override");
    assert_eq!(
        store.load("aurora").expect("bundle").0.meta.author,
        "Glowtune"
    );
}

#[test]
fn import_avoids_bundled_and_user_collisions() {
    let (dir, store) = store();
    let source = dir.path().join("source.toml");
    fs::write(&source, named("Aurora").to_toml_string()).expect("source");
    assert_eq!(store.import(&source).expect("import"), "aurora-2");
    assert_eq!(store.import(&source).expect("import again"), "aurora-3");
    assert_eq!(store.load("aurora-2").expect("load").0.meta.name, "Aurora");
}

#[test]
fn import_rejects_syntax_but_recovers_bad_fields() {
    let (dir, store) = store();
    let source = dir.path().join("source.toml");
    fs::write(&source, "[oops").expect("write");
    assert!(matches!(store.import(&source), Err(Error::TomlSyntax(_))));
    fs::write(
        &source,
        "[meta]\nname = 'Recovered'\n[glow]\nthickness = 'bad'",
    )
    .expect("write");
    assert_eq!(store.import(&source).expect("recover"), "recovered");
    assert_eq!(store.load("recovered").expect("load").0.glow.thickness, 56);
}

#[test]
fn import_suffix_preserves_id_length_limit() {
    let (dir, store) = store();
    let source = dir.path().join("source.toml");
    fs::write(&source, named(&"x".repeat(80)).to_toml_string()).expect("write");
    assert_eq!(store.import(&source).expect("first").len(), 48);
    let id = store.import(&source).expect("second");
    assert_eq!(id.len(), 48);
    assert!(id.ends_with("-2"));
}

#[test]
fn export_bundled_and_user_theme_can_replace_destination() {
    let (dir, store) = store();
    let dest = dir.path().join("export.toml");
    store.export("ember", &dest).expect("export bundle");
    assert_eq!(
        parse(&fs::read_to_string(&dest).expect("read")).0.meta.name,
        "Ember"
    );
    store.save(&named("Custom")).expect("save");
    store.export("custom", &dest).expect("replace export");
    assert_eq!(
        parse(&fs::read_to_string(dest).expect("read")).0.meta.name,
        "Custom"
    );
}

#[test]
fn delete_user_and_reject_bundled_deletion() {
    let (_dir, store) = store();
    store.save(&named("Custom")).expect("save");
    store.delete("custom").expect("delete");
    assert!(matches!(store.load("custom"), Err(Error::NotFound(_))));
    assert!(matches!(store.delete("custom"), Err(Error::NotFound(_))));
    assert!(matches!(
        store.delete("ember"),
        Err(Error::BundledReadOnly(_))
    ));
}

#[test]
fn ids_block_path_traversal_on_every_id_operation() {
    let (dir, store) = store();
    for id in [
        "", "../ember", "..", "a/b", "a\\b", "C:foo", "Aurora", "a.toml", "a\0b", "界", "/tmp/foo",
    ] {
        assert!(matches!(store.load(id), Err(Error::InvalidId(_))), "{id}");
        assert!(matches!(store.delete(id), Err(Error::InvalidId(_))), "{id}");
        assert!(
            matches!(
                store.export(id, dir.path().join("export")),
                Err(Error::InvalidId(_))
            ),
            "{id}"
        );
    }
    assert!(!dir.path().join("export").exists());
}

#[test]
fn io_errors_are_returned() {
    let (dir, store) = store();
    let file = dir.path().join("file");
    fs::write(&file, "").expect("write");
    assert!(matches!(ThemeStore::new(&file), Err(Error::Io(_))));
    assert!(matches!(
        store.import(dir.path().join("missing")),
        Err(Error::Io(_))
    ));
    fs::create_dir(dir.path().join("user/blocked.toml")).expect("directory");
    assert!(matches!(store.save(&named("Blocked")), Err(Error::Io(_))));
    assert!(matches!(store.load("blocked"), Err(Error::Io(_))));
    assert!(matches!(
        store.export("ember", dir.path().join("missing/out.toml")),
        Err(Error::Io(_))
    ));
}

#[test]
fn watcher_reports_written_file_within_three_seconds() {
    let dir = tempdir().expect("tempdir");
    let (sender, receiver) = mpsc::channel();
    let _watcher = ThemeWatcher::watch(dir.path(), move |id| {
        let _ = sender.send(id);
    })
    .expect("watch");
    fs::write(dir.path().join("custom.toml"), "[meta]\nname = 'Custom'").expect("write");
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("change"),
        "custom"
    );
}

#[test]
fn watcher_debounces_per_id_and_ignores_other_files() {
    let dir = tempdir().expect("tempdir");
    let (sender, receiver) = mpsc::channel();
    let _watcher = ThemeWatcher::watch(dir.path(), move |id| {
        let _ = sender.send(id);
    })
    .expect("watch");
    fs::write(dir.path().join("ignore.txt"), "").expect("write");
    fs::write(dir.path().join("Unsafe.toml"), "").expect("write");
    for index in 0..4 {
        fs::write(dir.path().join("one.toml"), index.to_string()).expect("write");
        std::thread::sleep(Duration::from_millis(25));
    }
    fs::write(dir.path().join("two.toml"), "").expect("write");
    let mut received = vec![
        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("first"),
        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("second"),
    ];
    received.sort();
    assert_eq!(received, ["one", "two"]);
    assert!(receiver.recv_timeout(Duration::from_millis(350)).is_err());
}

#[test]
fn watcher_reports_rename_and_drop_stops_promptly() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("staging.tmp"), "").expect("write");
    let (sender, receiver) = mpsc::channel();
    let watcher = ThemeWatcher::watch(dir.path(), move |id| {
        let _ = sender.send(id);
    })
    .expect("watch");
    fs::rename(
        dir.path().join("staging.tmp"),
        dir.path().join("renamed.toml"),
    )
    .expect("rename");
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("renamed"),
        "renamed"
    );
    let start = Instant::now();
    drop(watcher);
    assert!(start.elapsed() < Duration::from_secs(1));
    fs::write(dir.path().join("after.toml"), "").expect("write");
    assert!(receiver.recv_timeout(Duration::from_millis(300)).is_err());
}

#[test]
fn watcher_missing_directory_returns_error() {
    let dir = tempdir().expect("tempdir");
    assert!(matches!(
        ThemeWatcher::watch(dir.path().join("missing"), |_| {}),
        Err(Error::Io(_))
    ));
}

#[test]
fn watcher_reports_atomic_store_creates_and_replacements() {
    let dir = tempdir().expect("tempdir");
    let store = ThemeStore::new(dir.path()).expect("store");
    let (sender, receiver) = mpsc::channel();
    let _watcher = ThemeWatcher::watch(dir.path(), move |id| {
        let _ = sender.send(id);
    })
    .expect("watch");
    let mut theme = named("Live");
    store.save(&theme).expect("create");
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("create event"),
        "live"
    );
    theme.glow.enabled = false;
    store.save(&theme).expect("replace");
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("replacement event"),
        "live"
    );
    assert_eq!(store.load("live").expect("load").0, theme);
}
