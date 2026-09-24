use super::{
    escape_vdf_string, parse_vdf, set_persona_state_in_vdf, vdf_set_nested_value, vdf_tokenize_line,
};

#[test]
fn parse_vdf_extracts_multiple_accounts() {
    let content = r#""users"
{
"111"
{
    "AccountName"    "first"
    "PersonaName"    "First User"
    "Timestamp"      "123"
}
"222"
{
    "AccountName"    "second"
    "PersonaName"    "Second User"
    "Timestamp"      "456"
}
}"#;

    let parsed = parse_vdf(content);
    assert_eq!(parsed["111"]["accountname"], "first");
    assert_eq!(parsed["222"]["personaname"], "Second User");
    assert_eq!(parsed["222"]["timestamp"], "456");
}

#[test]
fn escapes_vdf_string_special_characters() {
    assert_eq!(
        escape_vdf_string(r#"+exec "autoexec.cfg" -path C:\Steam"#),
        r#"+exec \"autoexec.cfg\" -path C:\\Steam"#
    );
}

#[test]
fn set_nested_value_escapes_launch_options() {
    let input = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t}\n}\n";
    let output = vdf_set_nested_value(
        input,
        &["Software", "Valve", "Steam", "apps", "730", "LaunchOptions"],
        r#"+exec "autoexec.cfg" -path C:\Steam"#,
    )
    .expect("launch options must be written");

    assert!(
        output.contains("\"LaunchOptions\"\t\t\"+exec \\\"autoexec.cfg\\\" -path C:\\\\Steam\"")
    );
}

// ── V1: escape-aware tokenization ──

#[test]
fn tokenize_unescapes_quotes_and_backslashes() {
    // A value containing an escaped quote and an escaped backslash.
    let line = r#"	"LaunchOptions"		"+exec \"my cfg\" -path C:\\Steam""#;
    let tokens = vdf_tokenize_line(line);
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0], "LaunchOptions");
    assert_eq!(tokens[1], r#"+exec "my cfg" -path C:\Steam"#);
}

#[test]
fn tokenize_handles_escaped_newline() {
    let line = r#"	"key"		"line one\nline two""#;
    let tokens = vdf_tokenize_line(line);
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[1], "line one\nline two");
}

#[test]
fn parse_vdf_keeps_value_with_escaped_quote() {
    // The old split('"') tokenizer truncated this PersonaName at the
    // escaped quote. The escape-aware scanner must round-trip it.
    let content = "\"users\"\n{\n\t\"111\"\n\t{\n\t\t\"AccountName\"\t\"acct\"\n\t\t\"PersonaName\"\t\"say \\\"hi\\\" now\"\n\t}\n}\n";
    let parsed = parse_vdf(content);
    assert_eq!(parsed["111"]["accountname"], "acct");
    assert_eq!(parsed["111"]["personaname"], r#"say "hi" now"#);
}

#[test]
fn parse_vdf_handles_inline_section_braces() {
    let content = r#""users" {
"111" {
    "AccountName" "first"
    "PersonaName" "First User"
}
"222" {
    "AccountName" "second"
}
}"#;

    let parsed = parse_vdf(content);
    assert_eq!(parsed["111"]["accountname"], "first");
    assert_eq!(parsed["111"]["personaname"], "First User");
    assert_eq!(parsed["222"]["accountname"], "second");
}

// ── V2: newline escaping prevents VDF injection ──

#[test]
fn escapes_vdf_string_newlines() {
    assert_eq!(escape_vdf_string("a\nb\r\nc"), "a\\nb\\r\\nc");
}

#[test]
fn newline_in_launch_options_cannot_inject_lines() {
    // A value with a newline + a forged key must stay on one logical line.
    let input = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t}\n}\n";
    let injection = "good\"\n\t\t\"PersonaState\"\t\t\"7";
    let output = vdf_set_nested_value(
        input,
        &["Software", "Valve", "Steam", "apps", "730", "LaunchOptions"],
        injection,
    )
    .expect("launch options must be written");

    // The newline is escaped, so no second physical line is produced and
    // no real PersonaState key leaks into the file.
    assert!(output.contains("good\\\"\\n\t\t\\\"PersonaState\\\"\t\t\\\"7"));
    for line in output.lines() {
        let t = line.trim();
        assert!(
            !(t.starts_with("\"PersonaState\"")),
            "injection produced a real PersonaState line: {line}"
        );
    }
}

// ── V3: structural PersonaState targeting ──

const LOCALCONFIG: &str = "\"UserLocalConfigStore\"\n\
{\n\
\t\"friends\"\n\
\t{\n\
\t\t\"PersonaState\"\t\t\"1\"\n\
\t\t\"76561198000000000\"\n\
\t\t{\n\
\t\t\t\"name\"\t\t\"my PersonaState buddy\"\n\
\t\t\t\"PersonaState\"\t\t\"5\"\n\
\t\t}\n\
\t}\n\
}\n";

#[test]
fn set_persona_state_targets_friends_section() {
    let out = set_persona_state_in_vdf(LOCALCONFIG, "7").expect("should find PersonaState");
    // The direct friends.PersonaState changed to 7.
    assert!(out.contains("\t\t\"PersonaState\"\t\t\"7\"\n"));
    // The nested friend-block PersonaState (decoy) is untouched.
    assert!(out.contains("\t\t\t\"PersonaState\"\t\t\"5\"\n"));
    // The friend nickname mentioning PersonaState is untouched.
    assert!(out.contains("\"my PersonaState buddy\""));
}

#[test]
fn set_persona_state_ignores_decoy_outside_friends() {
    // A custom category named "PersonaState" sitting in another section
    // must not be hit.
    let content = "\"UserLocalConfigStore\"\n\
{\n\
\t\"WebStorage\"\n\
\t{\n\
\t\t\"PersonaState\"\t\t\"decoy\"\n\
\t}\n\
\t\"friends\"\n\
\t{\n\
\t\t\"PersonaState\"\t\t\"1\"\n\
\t}\n\
}\n";
    let out = set_persona_state_in_vdf(content, "0").expect("should find friends PersonaState");
    // WebStorage decoy untouched.
    assert!(out.contains("\t\t\"PersonaState\"\t\t\"decoy\"\n"));
    // friends PersonaState set to 0.
    assert!(out.contains("\t\t\"PersonaState\"\t\t\"0\"\n"));
}

#[test]
fn set_persona_state_returns_none_when_absent() {
    let content = "\"UserLocalConfigStore\"\n{\n\t\"friends\"\n\t{\n\t}\n}\n";
    assert!(set_persona_state_in_vdf(content, "1").is_none());
}

// ── V4: existing-key replace branch (hit on every Linux/macOS switch) ──

#[test]
fn set_nested_value_replaces_existing_key_in_place() {
    // loginusers.vdf-shaped content where the target key already exists.
    // set_login_user_flags hits this replace branch on every switch, so it
    // must swap the value without duplicating the key or touching siblings.
    let input = "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"0\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t}\n}\n";
    let output = vdf_set_nested_value(input, &["76561198000000000", "AllowAutoLogin"], "1")
        .expect("existing key must be replaced");

    // Value replaced in place.
    assert!(output.contains("\"AllowAutoLogin\"\t\t\"1\""));
    assert!(!output.contains("\"AllowAutoLogin\"\t\t\"0\""));
    // Key not duplicated.
    assert_eq!(output.matches("\"AllowAutoLogin\"").count(), 1);
    // Sibling keys untouched.
    assert!(output.contains("\"AccountName\"\t\t\"alice\""));
    assert!(output.contains("\"MostRecent\"\t\t\"0\""));
}

// ── V5: PersonaState value rewrite targets its own token ──

#[test]
fn set_persona_state_rewrites_only_its_own_value() {
    // Two quoted pairs crammed onto one physical line inside friends. The
    // old rfind('"') scan would have edited the trailing token instead.
    let content = "\"UserLocalConfigStore\"\n\
{\n\
\t\"friends\"\n\
\t{\n\
\t\t\"PersonaState\"\t\t\"1\"\t\t\"LastSeenState\"\t\t\"0\"\n\
\t}\n\
}\n";
    let out = set_persona_state_in_vdf(content, "7").expect("should find PersonaState");
    assert!(out.contains("\"PersonaState\"\t\t\"7\""));
    // The unrelated trailing token is NOT corrupted.
    assert!(out.contains("\"LastSeenState\"\t\t\"0\""));
}

// ── V6: a comment between a header and its brace does not desync ──

#[test]
fn set_persona_state_survives_comment_before_brace() {
    let content = "\"UserLocalConfigStore\"\n\
{\n\
\t\"friends\"\n\
\t// legacy note\n\
\t{\n\
\t\t\"PersonaState\"\t\t\"1\"\n\
\t}\n\
}\n";
    let out = set_persona_state_in_vdf(content, "7").expect("comment must not drop the edit");
    assert!(out.contains("\"PersonaState\"\t\t\"7\""));
}

// ── V7: CRLF files stay CRLF ──

#[test]
fn set_persona_state_preserves_crlf() {
    let content = "\"UserLocalConfigStore\"\r\n{\r\n\t\"friends\"\r\n\t{\r\n\t\t\"PersonaState\"\t\t\"1\"\r\n\t}\r\n}\r\n";
    let out = set_persona_state_in_vdf(content, "7").expect("should find PersonaState");
    assert!(out.contains("\"PersonaState\"\t\t\"7\"\r\n"));
    assert!(
        !out.contains("\"7\"\n\t}"),
        "line ending collapsed to bare LF"
    );
}

// ── V8: the write path is structural, like the read path ──
//
// Golden strings captured from the previous (brace-on-its-own-line only)
// implementation before it was replaced. A standalone-brace file must come
// back byte for byte the same, or this fix has changed files it had no
// business changing.

#[test]
fn set_nested_value_standalone_braces_are_byte_identical() {
    let cases: [(&str, &[&str], &str, &str); 5] = [
        // Replace an existing key in a loginusers.vdf-shaped file.
        (
            "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"0\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t}\n}\n",
            &["76561198000000000", "AllowAutoLogin"],
            "1",
            "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"1\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t}\n}\n",
        ),
        // Insert a key into the empty registry.vdf template.
        (
            "\"Registry\"\n{\n\t\"HKCU\"\n\t{\n\t\t\"Software\"\n\t\t{\n\t\t\t\"Valve\"\n\t\t\t{\n\t\t\t\t\"Steam\"\n\t\t\t\t{\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n",
            &["HKCU", "Software", "Valve", "Steam", "AutoLoginUser"],
            "alice",
            "\"Registry\"\n{\n\t\"HKCU\"\n\t{\n\t\t\"Software\"\n\t\t{\n\t\t\t\"Valve\"\n\t\t\t{\n\t\t\t\t\"Steam\"\n\t\t\t\t{\n\t\t\t\t\t\"AutoLoginUser\"\t\t\"alice\"\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n",
        ),
        // Create four missing sections plus the key.
        (
            "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t}\n}\n",
            &["Software", "Valve", "Steam", "apps", "730", "LaunchOptions"],
            "+exec \"autoexec.cfg\" -path C:\\Steam",
            "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"730\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"+exec \\\"autoexec.cfg\\\" -path C:\\\\Steam\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n",
        ),
        // Insert a missing key into a section that exists.
        (
            "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t}\n}\n",
            &["76561198000000000", "AllowAutoLogin"],
            "1",
            "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"1\"\n\t}\n}\n",
        ),
        // A file with no trailing newline keeps none.
        (
            "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t}\n}",
            &["76561198000000000", "AllowAutoLogin"],
            "1",
            "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"1\"\n\t}\n}",
        ),
    ];

    for (index, (input, path, value, expected)) in cases.iter().enumerate() {
        let out = vdf_set_nested_value(input, path, value).expect("golden case must succeed");
        assert_eq!(&out, expected, "golden case {index} drifted");
    }
}

#[test]
fn set_nested_value_enters_inline_brace_sections() {
    // `"key" {` on one line. The old writer walked straight past it, never
    // entered the block, and returned the input unchanged with Ok.
    let input = "\"users\" {\n\t\"76561198000000000\" {\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"0\"\n\t}\n}\n";
    let out = vdf_set_nested_value(input, &["76561198000000000", "AllowAutoLogin"], "1")
        .expect("inline braces must be walked");

    assert_eq!(
        out,
        "\"users\" {\n\t\"76561198000000000\" {\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"AllowAutoLogin\"\t\t\"1\"\n\t}\n}\n"
    );
}

#[test]
fn set_nested_value_inserts_into_an_inline_brace_section() {
    let input =
        "\"users\" {\n\t\"76561198000000000\" {\n\t\t\"AccountName\"\t\t\"alice\"\n\t}\n}\n";
    let out = vdf_set_nested_value(input, &["76561198000000000", "MostRecent"], "1")
        .expect("inline braces must be walked");

    assert_eq!(
        out,
        "\"users\" {\n\t\"76561198000000000\" {\n\t\t\"AccountName\"\t\t\"alice\"\n\t\t\"MostRecent\"\t\t\"1\"\n\t}\n}\n"
    );
}

#[test]
fn set_nested_value_splits_a_section_opened_and_closed_on_one_line() {
    // Both braces on the header line: the key cannot go in front of the
    // line, it has to land between them.
    let input = "\"UserLocalConfigStore\"\n{\n\t\"friends\" { }\n}\n";
    let out = vdf_set_nested_value(input, &["friends", "DoNotDisturb"], "1")
        .expect("inline section must accept the key");

    assert_eq!(
        out,
        "\"UserLocalConfigStore\"\n{\n\t\"friends\" {\n\t\t\"DoNotDisturb\"\t\t\"1\"\n\t}\n}\n"
    );
}

#[test]
fn set_nested_value_preserves_crlf() {
    let input = "\"users\"\r\n{\r\n\t\"76561198000000000\"\r\n\t{\r\n\t\t\"AllowAutoLogin\"\t\t\"0\"\r\n\t}\r\n}\r\n";
    let out = vdf_set_nested_value(input, &["76561198000000000", "AllowAutoLogin"], "1")
        .expect("CRLF file must be written");

    assert_eq!(
        out,
        "\"users\"\r\n{\r\n\t\"76561198000000000\"\r\n\t{\r\n\t\t\"AllowAutoLogin\"\t\t\"1\"\r\n\t}\r\n}\r\n"
    );
    assert!(!out.contains("\"1\"\n"), "line ending collapsed to bare LF");
}

#[test]
fn set_nested_value_preserves_space_indentation() {
    let input = "\"users\"\n{\n  \"76561198000000000\"\n  {\n  }\n}\n";
    let out = vdf_set_nested_value(input, &["76561198000000000", "MostRecent"], "1")
        .expect("space-indented file must be written");

    assert_eq!(
        out,
        "\"users\"\n{\n  \"76561198000000000\"\n  {\n    \"MostRecent\"\t\t\"1\"\n  }\n}\n"
    );
}

#[test]
fn set_nested_value_errors_when_the_path_cannot_be_reached() {
    // Empty file: nothing to walk, nowhere to put the key. The old writer
    // answered Ok("") and every caller wrote that back happily.
    let err = vdf_set_nested_value("", &["friends", "DoNotDisturb"], "1")
        .expect_err("an empty file has no path to write into");
    assert!(
        err.to_string().contains("friends > DoNotDisturb"),
        "the error must name the path: {err}"
    );

    // A file whose root section never opens is the same story.
    assert!(vdf_set_nested_value(
        "\"UserLocalConfigStore\"\n",
        &["friends", "DoNotDisturb"],
        "1"
    )
    .is_err());
}

#[test]
fn set_nested_value_escapes_quotes_and_backslashes_in_the_value() {
    let input =
        "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AccountName\"\t\t\"alice\"\n\t}\n}\n";
    let out = vdf_set_nested_value(
        input,
        &["76561198000000000", "Nickname"],
        "say \"hi\" from C:\\Steam",
    )
    .expect("value must be written");

    assert!(out.contains("\"Nickname\"\t\t\"say \\\"hi\\\" from C:\\\\Steam\""));
    // Reading it back through the tokenizer round-trips the raw value.
    let line = out
        .lines()
        .find(|l| l.trim_start().starts_with("\"Nickname\""))
        .expect("the key was written");
    assert_eq!(
        vdf_tokenize_line(line)[1],
        "say \"hi\" from C:\\Steam",
        "escaping must round-trip"
    );
}

#[test]
fn set_nested_value_keeps_a_second_pair_on_the_same_line() {
    // Two pairs crammed onto one physical line: rewriting the whole line
    // would silently drop the second one.
    let input = "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"AllowAutoLogin\"\t\t\"0\"\t\t\"MostRecent\"\t\t\"0\"\n\t}\n}\n";
    let out = vdf_set_nested_value(input, &["76561198000000000", "AllowAutoLogin"], "1")
        .expect("value must be written");

    assert!(out.contains("\"AllowAutoLogin\"\t\t\"1\""));
    assert!(out.contains("\"MostRecent\"\t\t\"0\""));
}
