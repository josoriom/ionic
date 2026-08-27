use hashbrown::HashMap;
use once_cell::sync::Lazy;

static RAW_JSON: &str = include_str!("cv_table.json");

static TABLE: Lazy<HashMap<String, String>> = Lazy::new(|| {
    let entries: std::collections::HashMap<String, String> =
        serde_json::from_str(RAW_JSON).unwrap();
    entries.into_iter().collect()
});

#[derive(Clone, Copy)]
pub(crate) struct CvTerm {
    pub accession: &'static str,
    pub name: &'static str,
}

pub(crate) fn find_term(accession: &str) -> Option<CvTerm> {
    TABLE.get_key_value(accession).map(|(key, name)| CvTerm {
        accession: key.as_str(),
        name: name.as_str(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_term_gets_name_for_known_accession() {
        let term = find_term("MS:1000511").expect("ms level is in the table");
        assert_eq!(term.accession, "MS:1000511");
        assert_eq!(term.name, "ms level");
    }

    #[test]
    fn find_term_gets_nothing_for_unknown_accession() {
        assert!(find_term("MS:9999999").is_none());
        assert!(find_term("").is_none());
        assert!(find_term("not an accession").is_none());
    }

    #[test]
    fn find_term_borrows_from_the_table_not_from_the_input() {
        let input = String::from("MS:1000511");
        let term = find_term(&input).expect("ms level is in the table");
        assert!(!std::ptr::eq(term.accession.as_ptr(), input.as_ptr()));
    }

    #[test]
    fn find_term_gets_the_same_text_every_time() {
        let first = find_term("MS:1000016").expect("scan start time is in the table");
        let second = find_term("MS:1000016").expect("scan start time is in the table");
        assert!(std::ptr::eq(first.name.as_ptr(), second.name.as_ptr()));
    }
}
