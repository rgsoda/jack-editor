//! What gets sent to a program that answers questions about code, and what to
//! make of what comes back.
//!
//! The program is whatever `:set aiprg=` names - `claude -p`, `llm`, `ollama
//! run`, anything that reads a prompt on stdin and writes an answer on
//! stdout. Nothing here knows which one it is, and nothing here talks to a
//! network: this module builds a string and reads a string, and the editor
//! does the rest.
//!
//! Everything in it is a pure function on purpose. What leaves your machine is
//! worth being able to test exactly.

/// Where the cursor is, and what is around it. The parts a program cannot
/// guess and would otherwise have to be told in prose.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct Context {
    /// The file, as the status line writes it.
    pub path: String,
    /// The line the cursor is on, counting from one.
    pub line: usize,
    /// The lines being sent, counting from one, when they are a range rather
    /// than whatever happened to be around the cursor.
    pub range: Option<(usize, usize)>,
    /// The code itself: the selected lines, or the item the cursor is in.
    pub code: String,
    /// What the language server or the last build said about that line.
    pub trouble: Option<String>,
}

/// What to send. `replacing` is the difference between "rewrite this" and
/// "tell me about this", which is the difference between an answer that is
/// pasted into the buffer and one that is read.
#[derive(Debug, PartialEq, Eq)]
pub struct Ask<'a> {
    pub instruction: &'a str,
    pub context: Option<Context>,
    pub replacing: bool,
}

/// The prompt, in the order a person would say it: what you asked first, then
/// what it is about.
///
/// A rewrite says what the answer has to look like, and says it after the
/// instruction rather than before: the last thing in a prompt is the thing
/// that gets followed.
pub fn prompt(ask: &Ask) -> String {
    let mut out = String::with_capacity(ask.instruction.len() + 256);
    out.push_str(ask.instruction.trim());
    out.push('\n');

    let Some(context) = &ask.context else {
        return out;
    };

    out.push('\n');
    match context.range {
        Some((first, last)) => {
            out.push_str(&format!("file: {}, lines {first}-{last}\n", context.path));
        }
        None => out.push_str(&format!("file: {}, line {}\n", context.path, context.line)),
    }
    if let Some(trouble) = &context.trouble {
        out.push_str(&format!("problem: {}\n", trouble.lines().next().unwrap_or_default().trim()));
    }

    if !context.code.trim().is_empty() {
        out.push('\n');
        out.push_str(match ask.replacing {
            true => "the lines:\n",
            false => "the code it is in:\n",
        });
        out.push_str(context.code.trim_end());
        out.push('\n');
    }

    if ask.replacing {
        out.push_str(
            "\nReply with the replacement for those lines and nothing else: \
             no explanation, no code fence, no line numbers. Keep the \
             indentation they had.\n",
        );
    }
    out
}

/// What came back, ready to go into the buffer.
///
/// Models put a code fence around code however plainly they are asked not to,
/// so the fence comes off here rather than being argued about in the prompt.
/// Only a fence that wraps the whole answer: one in the middle of an
/// explanation is part of the explanation.
pub fn cleaned(reply: &str) -> String {
    let body = reply.trim_matches('\n');
    if body.trim().is_empty() {
        return String::new();
    }
    let mut lines: Vec<&str> = body.lines().collect();
    let fenced = lines.first().is_some_and(|line| line.trim_start().starts_with("```"))
        && lines.len() > 1
        && lines.last().is_some_and(|line| line.trim() == "```");
    if fenced {
        lines.remove(0);
        lines.pop();
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// What to say about a program that said nothing, or could not be run at all.
/// `:make` has the same problem and the same answer: the last line it printed
/// beats silence.
pub fn complaint(command: &str, output: &str) -> String {
    match output.lines().rev().find(|line| !line.trim().is_empty()) {
        Some(last) => format!("{command}: {}", last.trim()),
        None => format!("{command} said nothing"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            path: "src/editor.rs".into(),
            line: 412,
            range: Some((412, 414)),
            code: "fn add(a: usize, b: usize) -> usize {\n    a - b\n}\n".into(),
            trouble: None,
        }
    }

    #[test]
    fn a_rewrite_says_what_the_answer_has_to_look_like() {
        let ask = Ask { instruction: "fix the arithmetic", context: Some(context()), replacing: true };
        let prompt = prompt(&ask);
        assert!(prompt.starts_with("fix the arithmetic\n"), "{prompt}");
        assert!(prompt.contains("file: src/editor.rs, lines 412-414"), "{prompt}");
        assert!(prompt.contains("    a - b"), "the code goes too");
        // And the instruction about the shape of the answer is last, which is
        // the part of a prompt that gets followed.
        let tail = prompt.rsplit_once("\n\n").expect("a last paragraph").1;
        assert!(tail.starts_with("Reply with the replacement"), "{tail}");
    }

    #[test]
    fn a_question_carries_where_you_are_and_what_is_wrong_there() {
        let mut context = context();
        context.range = None;
        context.trouble = Some("expected `usize`, found `u64`\nhelp: try `as usize`".into());
        let ask = Ask { instruction: "why does this not compile?", context: Some(context), replacing: false };
        let prompt = prompt(&ask);
        assert!(prompt.contains("file: src/editor.rs, line 412"), "{prompt}");
        // One line of it: the rest of a diagnostic is a wall, and the first
        // line is the complaint.
        assert!(prompt.contains("problem: expected `usize`, found `u64`\n"), "{prompt}");
        assert!(!prompt.contains("help: try"), "{prompt}");
        assert!(!prompt.contains("Reply with the replacement"), "nothing is being replaced");
    }

    #[test]
    fn a_bare_ask_is_your_words_and_nothing_else() {
        let ask = Ask { instruction: "what does `impl Trait` mean?", context: None, replacing: false };
        assert_eq!(prompt(&ask), "what does `impl Trait` mean?\n");
    }

    #[test]
    fn a_fence_round_the_whole_answer_comes_off() {
        assert_eq!(cleaned("```rust\nlet x = 1;\n```"), "let x = 1;\n");
        assert_eq!(cleaned("\n\n```\nlet x = 1;\n```\n\n"), "let x = 1;\n");
        // Not a fence that is part of what was said.
        let prose = "Like this:\n```\nlet x = 1;\n```\nand that is all.";
        assert_eq!(cleaned(prose), format!("{prose}\n"));
        // And code that was never fenced is left exactly as it is.
        assert_eq!(cleaned("    indented\n        more\n"), "    indented\n        more\n");
        assert_eq!(cleaned("   \n"), "");
    }

    #[test]
    fn a_program_that_said_nothing_is_still_worth_reporting() {
        assert_eq!(complaint("claude", ""), "claude said nothing");
        assert_eq!(complaint("claude", "\nboom: no such file\n\n"), "claude: boom: no such file");
    }
}
