/// Translate a LaTeX math string into Typst math syntax.
/// Returns the Typst content without surrounding `$` delimiters.
pub fn latex_to_typst_math(latex: &str) -> String {
    Translator::new(latex).translate()
}

struct Translator {
    chars: Vec<char>,
    pos: usize,
}

impl Translator {
    fn new(s: &str) -> Self {
        Self { chars: s.chars().collect(), pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t') | Some('\n') | Some('\r')) {
            self.advance();
        }
    }

    fn translate(&mut self) -> String {
        self.translate_until(None)
    }

    /// Translate characters until `stop` is found (which is consumed) or end of input.
    /// `{...}` in non-command context becomes `(...)` for Typst grouping.
    ///
    /// Inserts spaces between adjacent alphabetic tokens: in LaTeX, consecutive
    /// letters are implicitly multiplied separate variables; in Typst math they
    /// would be parsed as a single unknown identifier.
    fn translate_until(&mut self, stop: Option<char>) -> String {
        let mut out = String::new();
        loop {
            match self.peek() {
                None => break,
                Some(c) if Some(c) == stop => {
                    self.advance();
                    break;
                }
                Some('\\') => {
                    self.advance();
                    let result = self.translate_command();
                    if math_needs_space(&out, &result) {
                        out.push(' ');
                    }
                    out.push_str(&result);
                }
                Some('{') => {
                    self.advance();
                    let inner = self.translate_until(Some('}'));
                    out.push('(');
                    out.push_str(&inner);
                    out.push(')');
                }
                Some('}') if stop.is_none() => {
                    // Unmatched closing brace — stop here
                    self.advance();
                    break;
                }
                Some(c) if c.is_ascii_alphabetic() => {
                    // Add separator so adjacent letters are distinct Typst variables
                    if out.chars().last().map_or(false, |last| last.is_ascii_alphabetic()) {
                        out.push(' ');
                    }
                    out.push(c);
                    self.advance();
                }
                Some(c) => {
                    out.push(c);
                    self.advance();
                }
            }
        }
        out
    }

    fn read_command_name(&mut self) -> String {
        let mut name = String::new();
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() => {
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphabetic() {
                        name.push(c);
                        self.advance();
                    } else {
                        break;
                    }
                }
                // LaTeX consumes trailing whitespace after alphabetic command names
                while matches!(self.peek(), Some(' ') | Some('\t') | Some('\n')) {
                    self.advance();
                }
            }
            Some(c) => {
                name.push(c);
                self.advance();
            }
            None => {}
        }
        name
    }

    /// Parse a required `{arg}`, returning the translated content.
    /// If no `{` is present, reads a single character (or command).
    fn required_arg(&mut self) -> String {
        self.skip_whitespace();
        if self.peek() == Some('{') {
            self.advance();
            self.translate_until(Some('}'))
        } else if self.peek() == Some('\\') {
            self.advance();
            self.translate_command()
        } else {
            match self.advance() {
                Some(c) => c.to_string(),
                None => String::new(),
            }
        }
    }

    /// Try to parse an optional `[arg]`. Returns `None` if `[` is not next.
    fn optional_arg(&mut self) -> Option<String> {
        self.skip_whitespace();
        if self.peek() != Some('[') {
            return None;
        }
        self.advance();
        let mut content = String::new();
        let mut depth = 0i32;
        loop {
            match self.peek() {
                None => break,
                Some('[') => {
                    depth += 1;
                    content.push('[');
                    self.advance();
                }
                Some(']') => {
                    if depth == 0 {
                        self.advance();
                        break;
                    }
                    depth -= 1;
                    content.push(']');
                    self.advance();
                }
                Some(c) => {
                    content.push(c);
                    self.advance();
                }
            }
        }
        Some(content)
    }

    /// Collect raw (untranslated) characters until `\end{env}`, handling brace depth.
    /// Used so matrix/cases environments can be processed in a second pass.
    fn collect_until_end(&mut self, env: &str) -> String {
        let mut raw = String::new();
        let mut brace_depth = 0i32;
        loop {
            match self.peek() {
                None => break,
                Some('\\') => {
                    self.advance();
                    let cmd = self.read_command_name();
                    if cmd == "end" {
                        self.skip_whitespace();
                        if self.peek() == Some('{') {
                            self.advance();
                            let mut env_name = String::new();
                            while let Some(c) = self.peek() {
                                if c == '}' {
                                    self.advance();
                                    break;
                                }
                                env_name.push(c);
                                self.advance();
                            }
                            let expected = env.trim_end_matches('*');
                            let got = env_name.trim().trim_end_matches('*');
                            if got == expected || env_name.trim() == env {
                                break;
                            }
                            // Mismatched \end — put back
                            raw.push_str("\\end{");
                            raw.push_str(&env_name);
                            raw.push('}');
                        }
                    } else {
                        raw.push('\\');
                        raw.push_str(&cmd);
                    }
                }
                Some('{') => {
                    brace_depth += 1;
                    raw.push('{');
                    self.advance();
                }
                Some('}') => {
                    if brace_depth > 0 {
                        brace_depth -= 1;
                        raw.push('}');
                        self.advance();
                    } else {
                        break;
                    }
                }
                Some(c) => {
                    raw.push(c);
                    self.advance();
                }
            }
        }
        raw
    }

    fn translate_command(&mut self) -> String {
        let name = self.read_command_name();
        match name.as_str() {
            // ── Greek lowercase ──────────────────────────────────────────────
            "alpha" => "alpha".into(),
            "beta" => "beta".into(),
            "gamma" => "gamma".into(),
            "delta" => "delta".into(),
            "epsilon" => "epsilon".into(),
            "varepsilon" => "epsilon.alt".into(),
            "zeta" => "zeta".into(),
            "eta" => "eta".into(),
            "theta" => "theta".into(),
            "vartheta" => "theta.alt".into(),
            "iota" => "iota".into(),
            "kappa" => "kappa".into(),
            "lambda" => "lambda".into(),
            "mu" => "mu".into(),
            "nu" => "nu".into(),
            "xi" => "xi".into(),
            "pi" => "pi".into(),
            "varpi" => "pi.alt".into(),
            "rho" => "rho".into(),
            "varrho" => "rho.alt".into(),
            "sigma" => "sigma".into(),
            "varsigma" => "sigma.alt".into(),
            "tau" => "tau".into(),
            "upsilon" => "upsilon".into(),
            "phi" => "phi.alt".into(),
            "varphi" => "phi".into(),
            "chi" => "chi".into(),
            "psi" => "psi".into(),
            "omega" => "omega".into(),
            // ── Greek uppercase ──────────────────────────────────────────────
            "Gamma" => "Gamma".into(),
            "Delta" => "Delta".into(),
            "Theta" => "Theta".into(),
            "Lambda" => "Lambda".into(),
            "Xi" => "Xi".into(),
            "Pi" => "Pi".into(),
            "Sigma" => "Sigma".into(),
            "Upsilon" => "Upsilon".into(),
            "Phi" => "Phi".into(),
            "Psi" => "Psi".into(),
            "Omega" => "Omega".into(),
            // ── Symbols ─────────────────────────────────────────────────────
            "infty" => "infinity".into(),
            "hbar" => "planck.reduce".into(),
            "partial" => "partial".into(),
            "nabla" => "nabla".into(),
            "cdot" => "dot.op".into(),
            "times" => "times".into(),
            "div" => "div".into(),
            "pm" => "plus.minus".into(),
            "mp" => "minus.plus".into(),
            "leq" | "le" => "<=".into(),
            "geq" | "ge" => ">=".into(),
            "neq" | "ne" => "!=".into(),
            "approx" => "approx".into(),
            "equiv" => "equiv".into(),
            "sim" => "tilde.op".into(),
            "propto" => "prop".into(),
            "cong" => "tilde.equiv".into(),
            "perp" => "perp".into(),
            "parallel" => "parallel".into(),
            "to" | "rightarrow" => "arrow.r".into(),
            "leftarrow" | "gets" => "arrow.l".into(),
            "Rightarrow" => "arrow.r.double".into(),
            "Leftarrow" => "arrow.l.double".into(),
            "leftrightarrow" => "arrow.l.r".into(),
            "Leftrightarrow" => "arrow.l.r.double".into(),
            "uparrow" => "arrow.t".into(),
            "downarrow" => "arrow.b".into(),
            "mapsto" => "arrow.r.bar".into(),
            "in" => "in".into(),
            "notin" => "in.not".into(),
            "subset" => "subset".into(),
            "supset" => "supset".into(),
            "subseteq" => "subset.eq".into(),
            "supseteq" => "supset.eq".into(),
            "cup" => "union".into(),
            "cap" => "sect".into(),
            "setminus" => "without".into(),
            "emptyset" | "varnothing" => "nothing".into(),
            "forall" => "forall".into(),
            "exists" => "exists".into(),
            "neg" | "lnot" => "not".into(),
            "land" => "and".into(),
            "lor" => "or".into(),
            "langle" => "angle.l".into(),
            "rangle" => "angle.r".into(),
            "ket" => {
                let a = self.required_arg();
                format!("|{a} angle.r")
            }
            "bra" => {
                let a = self.required_arg();
                format!("angle.l {a}|")
            }
            "braket" => {
                let a = self.required_arg();
                format!("angle.l {a} angle.r")
            }
            "ldots" | "dots" => "...".into(),
            "cdots" => "dots.h".into(),
            "vdots" => "dots.v".into(),
            "ddots" => "dots.down".into(),
            "ell" => "ell".into(),
            "Re" => "Re".into(),
            "Im" => "Im".into(),
            "aleph" => "aleph".into(),
            "prime" => "prime".into(),
            "angle" => "angle".into(),
            "star" => "star".into(),
            "circ" => "circle.small".into(),
            "bullet" => "bullet".into(),
            "dagger" => "dagger".into(),
            "ddagger" => "dagger.double".into(),
            "oplus" => "plus.circle".into(),
            "otimes" => "times.circle".into(),
            "odot" => "dot.circle".into(),
            "ominus" => "minus.circle".into(),
            "oslash" => "div.circle".into(),
            // ── n-ary operators ──────────────────────────────────────────────
            "int" => "integral".into(),
            "iint" => "integral.double".into(),
            "iiint" => "integral.triple".into(),
            "oint" => "integral.cont".into(),
            "sum" => "sum".into(),
            "prod" => "product".into(),
            "coprod" => "product.co".into(),
            // ── Standard functions (no args, just the name) ──────────────────
            "lim" => "lim".into(),
            "limsup" => "limsup".into(),
            "liminf" => "liminf".into(),
            "inf" => "inf".into(),
            "sup" => "sup".into(),
            "max" => "max".into(),
            "min" => "min".into(),
            "det" => "det".into(),
            "exp" => "exp".into(),
            "log" => "log".into(),
            "ln" => "ln".into(),
            "sin" => "sin".into(),
            "cos" => "cos".into(),
            "tan" => "tan".into(),
            "cot" => "cot".into(),
            "sec" => "sec".into(),
            "csc" => "csc".into(),
            "sinh" => "sinh".into(),
            "cosh" => "cosh".into(),
            "tanh" => "tanh".into(),
            "arcsin" => "arcsin".into(),
            "arccos" => "arccos".into(),
            "arctan" => "arctan".into(),
            "gcd" => "gcd".into(),
            "ker" => "ker".into(),
            "dim" => "dim".into(),
            "deg" => "deg".into(),
            "hom" => "hom".into(),
            "arg" => "arg".into(),
            "Pr" => "Pr".into(),
            // ── Single-arg functions ─────────────────────────────────────────
            "sqrt" => {
                let opt = self.optional_arg();
                let arg = self.required_arg();
                match opt {
                    Some(n) => format!("root({n}, {arg})"),
                    None => format!("sqrt({arg})"),
                }
            }
            "hat" | "widehat" => {
                let a = self.required_arg();
                format!("hat({a})")
            }
            "check" => {
                let a = self.required_arg();
                format!("caron({a})")
            }
            "bar" | "overline" => {
                let a = self.required_arg();
                format!("macron({a})")
            }
            "underline" => {
                let a = self.required_arg();
                format!("underline({a})")
            }
            "tilde" | "widetilde" => {
                let a = self.required_arg();
                format!("tilde({a})")
            }
            "vec" => {
                let a = self.required_arg();
                format!("arrow({a})")
            }
            "dot" => {
                let a = self.required_arg();
                format!("dot({a})")
            }
            "ddot" => {
                let a = self.required_arg();
                format!("dot.double({a})")
            }
            "acute" => {
                let a = self.required_arg();
                format!("acute({a})")
            }
            "grave" => {
                let a = self.required_arg();
                format!("grave({a})")
            }
            "mathbf" | "boldsymbol" | "bm" => {
                let a = self.required_arg();
                format!("bold({a})")
            }
            "mathit" => {
                let a = self.required_arg();
                format!("italic({a})")
            }
            "mathrm" => {
                let a = self.required_arg();
                format!("upright({a})")
            }
            "operatorname" => {
                let a = self.required_arg();
                format!("op(\"{a}\")")
            }
            "text" | "mbox" => {
                let a = self.required_arg();
                format!("\"{a}\"")
            }
            "mathcal" => {
                let a = self.required_arg();
                format!("cal({a})")
            }
            "mathbb" => {
                let a = self.required_arg();
                match a.trim() {
                    "R" => "RR".into(),
                    "Z" => "ZZ".into(),
                    "N" => "NN".into(),
                    "Q" => "QQ".into(),
                    "C" => "CC".into(),
                    "H" => "HH".into(),
                    other => format!("bb({other})"),
                }
            }
            "mathfrak" => {
                let a = self.required_arg();
                format!("frak({a})")
            }
            "mathtt" => {
                let a = self.required_arg();
                format!("mono({a})")
            }
            "not" => {
                // \not\cmd — wrap the next symbol in cancel()
                self.skip_whitespace();
                if self.peek() == Some('\\') {
                    self.advance();
                    let sym = self.translate_command();
                    format!("cancel({sym})")
                } else {
                    "cancel".into()
                }
            }
            // ── Two-arg functions ────────────────────────────────────────────
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let num = self.required_arg();
                let den = self.required_arg();
                format!("frac({num}, {den})")
            }
            "binom" | "dbinom" => {
                let n = self.required_arg();
                let k = self.required_arg();
                format!("binom({n}, {k})")
            }
            "overset" => {
                let top = self.required_arg();
                let base = self.required_arg();
                format!("overset({top}, {base})")
            }
            "underset" => {
                let bot = self.required_arg();
                let base = self.required_arg();
                format!("underset({bot}, {base})")
            }
            "underbrace" => {
                let a = self.required_arg();
                format!("underbrace({a})")
            }
            "overbrace" => {
                let a = self.required_arg();
                format!("overbrace({a})")
            }
            "xleftarrow" | "xrightarrow" => {
                let _opt = self.optional_arg();
                let label = self.required_arg();
                let arr = if name.starts_with("xl") { "arrow.l" } else { "arrow.r" };
                format!("overset(\"{label}\", {arr})")
            }
            // ── Environments ─────────────────────────────────────────────────
            "begin" => {
                self.skip_whitespace();
                let env_name = if self.peek() == Some('{') {
                    self.advance();
                    let mut n = String::new();
                    while let Some(c) = self.peek() {
                        if c == '}' {
                            self.advance();
                            break;
                        }
                        n.push(c);
                        self.advance();
                    }
                    n
                } else {
                    String::new()
                };
                self.translate_environment(&env_name)
            }
            "end" => {
                // Consume env name — shouldn't normally appear at top level
                self.skip_whitespace();
                if self.peek() == Some('{') {
                    self.advance();
                    while let Some(c) = self.advance() {
                        if c == '}' {
                            break;
                        }
                    }
                }
                String::new()
            }
            // ── \left / \right ───────────────────────────────────────────────
            "left" | "right" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr"
            | "Biggl" | "Biggr" | "big" | "Big" | "bigg" | "Bigg" => {
                // Drop the sizing command; the bracket that follows emits naturally
                if self.peek() == Some('.') {
                    self.advance(); // \left. / \right. = invisible delimiter
                }
                String::new()
            }
            // ── Spacing ───────────────────────────────────────────────────────
            "," => "thin".into(),
            ";" | ":" => " ".into(),
            "!" => String::new(),
            " " => " ".into(),
            "quad" => "#h(1em)".into(),
            "qquad" => "#h(2em)".into(),
            // ── Style / display modifiers (ignored) ───────────────────────────
            "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" => String::new(),
            "limits" | "nolimits" | "displaylimits" => String::new(),
            "mathstrut" | "strut" | "vphantom" | "hphantom" | "phantom" => String::new(),
            "label" => {
                // Consume the label arg and drop it
                let _ = self.required_arg();
                String::new()
            }
            // ── Delimiters ────────────────────────────────────────────────────
            "mid" | "vert" => "|".into(),
            "Vert" => "||".into(),
            // ── Line break (in align environments) ───────────────────────────
            "\\" => " \\\n".into(),
            // ── Escaped special characters ────────────────────────────────────
            "{" => "{".into(),
            "}" => "}".into(),
            "[" => "[".into(),
            "]" => "]".into(),
            "(" => "(".into(),
            ")" => ")".into(),
            "|" => "|".into(),
            "&" => " & ".into(),
            "#" => "#".into(),
            "%" => "%".into(),
            "$" => "\\$".into(),
            // ── Unknown: pass through as-is (many names identical in Typst) ──
            other => split_typographic_partial_prefix(other).unwrap_or_else(|| other.into()),
        }
    }

    fn translate_environment(&mut self, env: &str) -> String {
        match env.trim_end_matches('*') {
            "matrix" | "pmatrix" | "bmatrix" | "vmatrix" | "Vmatrix" | "Bmatrix"
            | "smallmatrix" => {
                let raw = self.collect_until_end(env);
                translate_matrix_env(env, &raw)
            }
            "cases" | "dcases" => {
                let raw = self.collect_until_end(env);
                translate_cases_env(&raw)
            }
            _ => {
                // align, equation, gather, multline, etc.
                // Collect raw content and translate, stripping alignment markers
                let raw = self.collect_until_end(env);
                let cleaned = raw.replace(" & ", " ").replace('&', "");
                Translator::new(&cleaned).translate()
            }
        }
    }
}

/// `\partialt` (missing brace/space) is common in model output; Typst would parse
/// `partialt` as one undefined variable. Expand to `partial` + translated suffix.
fn split_typographic_partial_prefix(name: &str) -> Option<String> {
    const PREFIX: &str = "partial";
    if name == PREFIX {
        return None;
    }
    if !name.starts_with(PREFIX) || name.len() <= PREFIX.len() {
        return None;
    }
    let suffix = &name[PREFIX.len()..];
    if suffix.is_empty() || !suffix.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if suffix == PREFIX {
        return None;
    }
    if suffix.len() == 1 {
        let c = suffix.chars().next()?;
        return Some(format!("partial {c}"));
    }
    // Multi-letter suffix: treat as `\suffix` (e.g. `partialtheta` → partial theta).
    let inner = format!(r"\{}", suffix);
    let translated = Translator::new(&inner).translate();
    Some(format!("partial {translated}"))
}

/// Returns true if a space separator is needed between `before` output and the
/// upcoming `after` token to prevent them merging into a single Typst identifier.
fn math_needs_space(before: &str, after: &str) -> bool {
    let before_alpha = before.chars().last().map_or(false, |c| c.is_ascii_alphabetic());
    let after_alpha = after.chars().next().map_or(false, |c| c.is_ascii_alphabetic());
    before_alpha && after_alpha
}

fn translate_matrix_env(env: &str, raw: &str) -> String {
    // Split on \\ (two actual backslashes in raw) for rows, & for columns
    let row_strs: Vec<&str> = raw.split("\\\\").collect();
    let rows: Vec<Vec<String>> = row_strs
        .iter()
        .map(|row_str| {
            row_str
                .split('&')
                .map(|cell| Translator::new(cell.trim()).translate())
                .collect()
        })
        .collect();

    let content = rows
        .iter()
        .map(|row| row.join(", "))
        .collect::<Vec<_>>()
        .join("; ");

    let delim_arg = match env.trim_end_matches('*') {
        "pmatrix" => "",
        "bmatrix" => ", delim: \"[\"",
        "vmatrix" => ", delim: \"|\"",
        "Vmatrix" => ", delim: \"||\"",
        "Bmatrix" => ", delim: \"{\"",
        _ => "",
    };

    format!("mat({content}{delim_arg})")
}

fn translate_cases_env(raw: &str) -> String {
    let row_strs: Vec<&str> = raw.split("\\\\").collect();
    let cases: Vec<String> = row_strs
        .iter()
        .map(|row_str| {
            let parts: Vec<&str> = row_str.splitn(2, '&').collect();
            let main = Translator::new(parts[0].trim()).translate();
            if let Some(cond_raw) = parts.get(1) {
                let cond = Translator::new(cond_raw.trim()).translate();
                if cond.is_empty() {
                    main
                } else {
                    format!("{main} quad {cond}")
                }
            } else {
                main
            }
        })
        .collect();
    format!("cases({})", cases.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> String {
        latex_to_typst_math(s)
    }

    #[test]
    fn simple_expression_letters_spaced() {
        // Adjacent letters get a space so Typst treats them as separate variables
        // (LaTeX implicit multiplication) rather than one unknown identifier.
        assert_eq!(t("E = mc^2"), "E = m c^2");
    }

    #[test]
    fn greek_letters_translated() {
        assert_eq!(t("\\alpha"), "alpha");
        assert_eq!(t("\\Psi"), "Psi");
        assert_eq!(t("\\omega"), "omega");
    }

    #[test]
    fn hbar_translated() {
        assert_eq!(t("\\hbar"), "planck.reduce");
    }

    #[test]
    fn frac_two_args() {
        assert_eq!(t("\\frac{1}{2}"), "frac(1, 2)");
    }

    #[test]
    fn frac_with_expressions() {
        assert_eq!(t("\\frac{-b}{2a}"), "frac(-b, 2a)");
    }

    #[test]
    fn sqrt_no_opt() {
        assert_eq!(t("\\sqrt{x}"), "sqrt(x)");
    }

    #[test]
    fn sqrt_with_opt() {
        assert_eq!(t("\\sqrt[3]{x}"), "root(3, x)");
    }

    #[test]
    fn nested_frac() {
        assert_eq!(t("\\frac{\\frac{a}{b}}{c}"), "frac(frac(a, b), c)");
    }

    #[test]
    fn hat_accent() {
        assert_eq!(t("\\hat{H}"), "hat(H)");
    }

    #[test]
    fn mathbf_bold() {
        assert_eq!(t("\\mathbf{r}"), "bold(r)");
    }

    #[test]
    fn mathbb_real_numbers() {
        assert_eq!(t("\\mathbb{R}"), "RR");
        assert_eq!(t("\\mathbb{Z}"), "ZZ");
    }

    #[test]
    fn arrows_and_relations() {
        assert_eq!(t("\\to"), "arrow.r");
        assert_eq!(t("\\leq"), "<=");
        assert_eq!(t("\\neq"), "!=");
    }

    #[test]
    fn brace_grouping_becomes_parens() {
        assert_eq!(t("x^{n+1}"), "x^(n+1)");
    }

    #[test]
    fn left_right_dropped() {
        assert_eq!(t("\\left(x\\right)"), "(x)");
    }

    #[test]
    fn schrodinger_equation() {
        let latex = r"i\hbar\frac{\partial}{\partial t}|\Psi\rangle = \hat{H}|\Psi\rangle";
        let out = t(latex);
        assert!(out.contains("planck.reduce"), "missing hbar: {out}");
        assert!(out.contains("frac(partial, partial t)"), "missing frac: {out}");
        assert!(out.contains("angle.r"), "missing rangle: {out}");
        assert!(out.contains("hat(H)"), "missing hat H: {out}");
    }

    #[test]
    fn partial_t_concatenated_command_expands() {
        // LLMs often emit `\partialt`; Typst errors on unknown variable `partialt`.
        assert_eq!(t(r"\partialt"), "partial t");
    }

    #[test]
    fn partial_theta_concatenated_command_expands() {
        assert_eq!(t(r"\partialtheta"), "partial theta");
    }
}
