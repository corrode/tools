fn unescape_html(html_string: &str) -> String {
    let replacements = [
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&apos;", "'"),
        ("&cent;", "¢"),
        ("&pound;", "£"),
        ("&yen;", "¥"),
        ("&euro;", "€"),
        ("&copy;", "©"),
        ("&reg;", "®"),
        ("&sect;", "§"),
        ("&uml;", "¨"),
        ("&ordf;", "ª"),
        ("&laquo;", "«"),
        ("&not;", "¬"),
        ("&shy;", "­"),
        ("&macr;", "¯"),
        ("&deg;", "°"),
        ("&plusmn;", "±"),
        ("&sup2;", "²"),
        ("&sup3;", "³"),
        ("&acute;", "´"),
        ("&micro;", "µ"),
        ("&para;", "¶"),
        ("&middot;", "·"),
        ("&cedil;", "¸"),
        ("&sup1;", "¹"),
        ("&ordm;", "º"),
        ("&raquo;", "»"),
        ("&frac14;", "¼"),
        ("&frac12;", "½"),
        ("&frac34;", "¾"),
        ("&iquest;", "¿"),
        ("&Agrave;", "À"),
        ("&Aacute;", "Á"),
        ("&Acirc;", "Â"),
        ("&Atilde;", "Ã"),
        ("&Auml;", "Ä"),
        ("&Aring;", "Å"),
        ("&AElig;", "Æ"),
        ("&Ccedil;", "Ç"),
        ("&Egrave;", "È"),
        ("&Eacute;", "É"),
        ("&Ecirc;", "Ê"),
        ("&Euml;", "Ë"),
        ("&Igrave;", "Ì"),
        ("&Iacute;", "Í"),
        ("&Icirc;", "Î"),
        ("&Iuml;", "Ï"),
        ("&ETH;", "Ð"),
        ("&Ntilde;", "Ñ"),
        ("&Ograve;", "Ò"),
        ("&Oacute;", "Ó"),
        ("&Ocirc;", "Ô"),
        ("&Otilde;", "Õ"),
        ("&Ouml;", "Ö"),
        ("&times;", "×"),
        ("&Oslash;", "Ø"),
        ("&Ugrave;", "Ù"),
        ("&Uacute;", "Ú"),
        ("&Ucirc;", "Û"),
        ("&Uuml;", "Ü"),
        ("&Yacute;", "Ý"),
        ("&THORN;", "Þ"),
        ("&szlig;", "ß"),
        ("&agrave;", "à"),
        ("&aacute;", "á"),
        ("&acirc;", "â"),
        ("&atilde;", "ã"),
        ("&auml;", "ä"),
        ("&aring;", "å"),
        ("&aelig;", "æ"),
        ("&ccedil;", "ç"),
        ("&egrave;", "è"),
        ("&eacute;", "é"),
        ("&ecirc;", "ê"),
        ("&euml;", "ë"),
        ("&igrave;", "ì"),
        ("&iacute;", "í"),
        ("&icirc;", "î"),
        ("&iuml;", "ï"),
        ("&eth;", "ð"),
        ("&ntilde;", "ñ"),
        ("&ograve;", "ò"),
        ("&oacute;", "ó"),
        ("&ocirc;", "ô"),
        ("&otilde;", "õ"),
        ("&ouml;", "ö"),
        ("&divide;", "÷"),
        ("&oslash;", "ø"),
        ("&ugrave;", "ù"),
        ("&uacute;", "ú"),
        ("&ucirc;", "û"),
        ("&uuml;", "ü"),
        ("&yacute;", "ý"),
        ("&thorn;", "þ"),
        ("&yuml;", "ÿ"),
    ];
    replacements
        .iter()
        .fold(html_string.to_string(), |acc, &(entity, char)| {
            acc.replace(entity, char)
        })
}

fn replace_abbreviations(text: &str) -> String {
    let abbreviations = [
        ("i.e.", "ie"),
        ("e.g.", "eg"),
        ("etc.", "etc"),
        ("mr.", "mr"),
        ("mrs.", "mrs"),
        ("vs.", "vs"),
        ("dr.", "dr"),
        ("prof.", "prof"),
        ("sr.", "sr"),
        ("jr.", "jr"),
        ("st.", "st"),
        ("jan.", "jan"),
        ("feb.", "feb"),
        ("mar.", "mar"),
        ("apr.", "apr"),
        ("jun.", "jun"),
        ("jul.", "jul"),
        ("aug.", "aug"),
        ("sept.", "sept"),
        ("oct.", "oct"),
        ("nov.", "nov"),
        ("dec.", "dec"),
        ("a.m.", "am"),
        ("p.m.", "pm"),
        ("u.s.", "us"),
        ("u.k.", "uk"),
    ];
    let regex_set = regex::RegexSet::new(abbreviations.iter().map(|&(abbr, _)| abbr)).unwrap();
    regex_set
        .matches(text)
        .iter()
        .fold(text.to_string(), |acc, m| {
            let (from, to) = abbreviations[m];
            acc.replace(from, to)
        })
}

fn remove_html_tags(html_string: &str) -> String {
    let text = regex::Regex::new(r"(?s)<!--(.*?)-->")
        .unwrap()
        .replace_all(html_string, "")
        .into_owned();

    let text = regex::Regex::new(r"(?s)<h[1-6]>(.*?)</h[1-6]>")
        .unwrap()
        .replace_all(&text, "$1\n\n")
        .into_owned();

    let text = unescape_html(&text);
    let text = regex::Regex::new(r"<(.*?)>")
        .unwrap()
        .replace_all(&text, " ")
        .into_owned();
    let text = regex::Regex::new(r"  ")
        .unwrap()
        .replace_all(&text, " ")
        .into_owned();
    let text = replace_abbreviations(&text);
    let text = regex::Regex::new(r"\n\s*?\n")
        .unwrap()
        .replace_all(&text, "\n\n")
        .into_owned();
    let text = regex::Regex::new(r"\s?\[[0-9]+\]\s?")
        .unwrap()
        .replace_all(&text, "")
        .into_owned();
    let text = text
        .split("\n")
        .map(|line| line.trim())
        .filter(|line| !line.starts_with("^  "))
        .collect::<Vec<&str>>()
        .join("\n");
    // remove all sequences of 3 or more newlines with two newlines
    let text = regex::Regex::new(r"\n{3,}")
        .unwrap()
        .replace_all(&text, "\n\n")
        .into_owned();
    text
}

pub fn prepare_text(text: &str) -> String {
    let text = text
        .split("\n")
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<&str>>()
        .join(" ");

    let text = html2md::parse_html(&text);

    let text = remove_html_tags(&text);

    let paragraphs = cut(&text);

    paragraphs
        .iter()
        .map(|p| p.as_slice().join(" "))
        .collect::<Vec<String>>()
        .join("\n\n")
}

use regex::Regex;

fn remove_composite_abbreviations(text: &str) -> String {
    Regex::new(r"(?P<comp>et al\.)(?:\.)")
        .unwrap()
        .replace_all(text, "$comp&;&")
        .to_string()
}

fn remove_suspension_points(text: &str) -> String {
    Regex::new(r"\.{3}")
        .unwrap()
        .replace_all(text, "&&&.")
        .to_string()
}

fn remove_floating_point_numbers(text: &str) -> String {
    Regex::new(r"(?P<number>[0-9]+)\.(?P<decimal>[0-9]+)")
        .unwrap()
        .replace_all(text, "$number&@&$decimal")
        .to_string()
}

fn handle_floats_without_leading_zero(text: &str) -> String {
    Regex::new(r"\s\.(?P<nums>[0-9]+)")
        .unwrap()
        .replace_all(text, " &#&$nums")
        .to_string()
}

fn remove_abbreviations(text: &str) -> String {
    Regex::new(r"(?:[A-Za-z]\.){2,}")
        .unwrap()
        .replace_all(text, |caps: &regex::Captures| {
            caps.iter()
                .filter_map(|c| c.map(|c| c.as_str().to_string().replace(".", "&-&")))
                .collect::<String>()
        })
        .to_string()
}

fn remove_initials(text: &str) -> String {
    Regex::new(r"(?P<init>[A-Z])(?P<point>\.)")
        .unwrap()
        .replace_all(text, "$init&_&")
        .to_string()
}

fn remove_titles(text: &str) -> String {
    Regex::new(r"(?P<title>[A-Z][a-z]{1,3})(\.)")
        .unwrap()
        .replace_all(text, "$title&*&")
        .to_string()
}

fn unstick_sentences(text: &str) -> String {
    Regex::new(r##"(?P<left>[^.?!]\.|!|\?)(?P<right>[^\s"'])"##)
        .unwrap()
        .replace_all(text, "$left $right")
        .to_string()
}

fn remove_sentence_enders_before_parens(text: &str) -> String {
    Regex::new(r##"(?P<bef>[.?!])\s?\)"##)
        .unwrap()
        .replace_all(text, "&==&$bef")
        .to_string()
}

fn remove_sentence_enders_next_to_quotes(text: &str) -> String {
    let transformations = [
        (r##"'(?P<quote>[.?!])\s?""##, "&^&$quote"),
        (r##"'(?P<quote>[.?!])\s?”"##, "&**&$quote"),
        (r##"(?P<quote>[.?!])\s?”"##, "&=&$quote"),
        (r##"(?P<quote>[.?!])\s?'""##, "&,&$quote"),
        (r##"(?P<quote>[.?!])\s?'"##, "&##&$quote"),
        (r##"(?P<quote>[.?!])\s?""##, "&$quote"),
    ];
    transformations
        .iter()
        .fold(text.to_string(), |acc, (pattern, repl)| {
            Regex::new(pattern)
                .unwrap()
                .replace_all(&acc, *repl)
                .to_string()
        })
}

fn split_sentences(text: &str) -> Vec<Vec<String>> {
    let mut paragraphs: Vec<Vec<String>> = Vec::new();
    let mut current_sentence = String::new();
    let mut current_paragraph = Vec::new();

    for c in text.chars() {
        if c == '\n' {
            if !current_sentence.is_empty() {
                current_paragraph.push(current_sentence.clone());
                current_sentence.clear();
            }
            if !current_paragraph.is_empty() {
                paragraphs.push(current_paragraph.clone());
                current_paragraph.clear();
            }
        } else {
            current_sentence.push(c);
            if c == '.' || c == '?' || c == '!' {
                current_paragraph.push(current_sentence.clone());
                current_sentence.clear();
            }
        }
    }

    if !current_sentence.is_empty() {
        current_paragraph.push(current_sentence);
    }
    if !current_paragraph.is_empty() {
        paragraphs.push(current_paragraph);
    }

    paragraphs
}

fn repair_sentences(paragraphs: Vec<Vec<String>>) -> Vec<Vec<String>> {
    let paren_repair = Regex::new(r"&==&(?P<p>[.!?])").unwrap();
    let quote_repair_regexes = [
        Regex::new(r"&\^&(?P<p>[.!?])").unwrap(),
        Regex::new(r"&\*\*&(?P<p>[.!?])").unwrap(),
        Regex::new(r"&=&(?P<p>[.!?])").unwrap(),
        Regex::new(r#"&,&(?P<p>[.!?])"#).unwrap(),
        Regex::new(r"&##&(?P<p>[.!?])").unwrap(),
        Regex::new(r"&\$&(?P<p>[.!?])").unwrap(),
    ];

    let repaired_paragraphs = paragraphs
        .into_iter()
        .map(|paragraph| {
            paragraph
                .into_iter()
                .map(|s| {
                    let replaced_sentence = s
                        .trim()
                        .replace("&;&", ".")
                        .replace("&&&", "..")
                        .replace("&@&", ".")
                        .replace("&#&", ".")
                        .replace("&-&", ".")
                        .replace("&_&", ".")
                        .replace("&*&", ".");
                    let paren_repaired = paren_repair
                        .replace_all(&replaced_sentence, r"$1)")
                        .to_string();
                    quote_repair_regexes
                        .iter()
                        .fold(paren_repaired, |acc, regex| {
                            regex
                                .replace_all(
                                    &acc,
                                    match regex as *const Regex {
                                        x if x == &quote_repair_regexes[0] as *const Regex => {
                                            r#"'$p""#
                                        }
                                        x if x == &quote_repair_regexes[1] as *const Regex => {
                                            r#"'$p”"#
                                        }
                                        x if x == &quote_repair_regexes[2] as *const Regex => {
                                            r#"$p”"#
                                        }
                                        x if x == &quote_repair_regexes[3] as *const Regex => {
                                            r#"$p""#
                                        }
                                        x if x == &quote_repair_regexes[4] as *const Regex => {
                                            r#"$p'"#
                                        }
                                        _ => r#"$p""#,
                                    },
                                )
                                .to_string()
                        })
                })
                .filter(|s| !s.is_empty())
                .collect()
        })
        .filter(|p: &Vec<String>| !p.is_empty())
        .collect();

    repaired_paragraphs
}

pub fn cut(origin_text: &String) -> Vec<Vec<String>> {
    let mut text = remove_composite_abbreviations(origin_text);
    text = remove_suspension_points(&text);
    text = remove_floating_point_numbers(&text);
    text = handle_floats_without_leading_zero(&text);
    text = remove_abbreviations(&text);
    text = remove_initials(&text);
    text = remove_titles(&text);
    text = unstick_sentences(&text);
    text = remove_sentence_enders_before_parens(&text);
    text = remove_sentence_enders_next_to_quotes(&text);
    let paragraphs = split_sentences(&text);
    repair_sentences(paragraphs)
}
