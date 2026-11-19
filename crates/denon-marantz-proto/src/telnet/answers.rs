//! Which written queries the receiver has answered.
//!
//! Telnet has no request ids, and the receiver sends every answer to every
//! connection. So a line answers the open query whose text it starts with. If
//! several match, the longest text wins, also among the queries that are not
//! open: `MSQUICK0` answers `MSQUICK ?`, not `MS?`. A list answer is open until
//! its `END` line.

use std::borrow::Cow;

use super::query::{is_list_query, known_answer_prefixes};
use crate::brand::Brand;

/// What a line from the receiver means for the written queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AnswerLine {
    /// The line answers an open query, or is a line of an open list.
    Answer,
    /// The line belongs to a query that was answered before, for example the
    /// second line of `TR?`.
    Related,
    /// The line has nothing to do with the written queries.
    Other,
}

/// Keeps the queries that are written and not answered yet.
#[derive(Debug, Clone)]
pub struct AnswerTracker {
    brand: Brand,
    open: Vec<Open>,
    /// Queries answered since the last [`AnswerTracker::forget_answered`].
    answered: Vec<Cow<'static, str>>,
}

#[derive(Debug, Clone)]
struct Open {
    query: Cow<'static, str>,
    list: bool,
}

impl AnswerTracker {
    /// A tracker for the queries of this brand.
    #[must_use]
    pub const fn new(brand: Brand) -> Self {
        Self {
            brand,
            open: Vec::new(),
            answered: Vec::new(),
        }
    }

    /// The query was written. It is open until a line answers it. A query
    /// that is only `?` asks nothing and is not kept.
    pub fn written(&mut self, query: Cow<'static, str>) {
        if answer_prefix(&query).is_empty() {
            return;
        }
        let list = is_list_query(&query);
        self.open.push(Open { query, list });
    }

    /// Checks a line from the receiver. It closes the open query that it
    /// answers. A list closes on its `END` line.
    pub fn line(&mut self, line: &str) -> AnswerLine {
        let open = self
            .open
            .iter()
            .enumerate()
            .filter(|(_, open)| line.starts_with(answer_prefix(&open.query)))
            .max_by_key(|(_, open)| answer_prefix(&open.query).len());
        if let Some((index, open)) = open {
            let prefix = answer_prefix(&open.query);
            if !self.longer_known(line, prefix.len()) {
                if !open.list || line[prefix.len()..].trim() == "END" {
                    let open = self.open.remove(index);
                    self.answered.push(open.query);
                }
                return AnswerLine::Answer;
            }
        }
        let related = self.answered.iter().any(|query| {
            let prefix = answer_prefix(query);
            line.starts_with(prefix) && !self.longer_known(line, prefix.len())
        });
        if related {
            AnswerLine::Related
        } else {
            AnswerLine::Other
        }
    }

    /// True if no written query waits for its answer.
    #[must_use]
    pub const fn is_idle(&self) -> bool {
        self.open.is_empty()
    }

    /// True if some queries were answered since the last
    /// [`AnswerTracker::forget_answered`].
    #[must_use]
    pub const fn has_answered(&self) -> bool {
        !self.answered.is_empty()
    }

    /// The open queries, oldest first. They stay open.
    pub fn open(&self) -> impl Iterator<Item = &Cow<'static, str>> {
        self.open.iter().map(|open| &open.query)
    }

    /// Removes the open queries and gives them, oldest first. Use it when the
    /// receiver was quiet for so long that they are not answered.
    pub fn take_open(&mut self) -> impl Iterator<Item = Cow<'static, str>> + '_ {
        self.open.drain(..).map(|open| open.query)
    }

    /// Forgets the answered queries, so that their lines are
    /// [`AnswerLine::Other`] again.
    pub fn forget_answered(&mut self) {
        self.answered.clear();
    }

    /// Forgets everything, for a new connection.
    pub fn clear(&mut self) {
        self.open.clear();
        self.answered.clear();
    }

    /// True if a known query with a longer text than `len` matches the line.
    fn longer_known(&self, line: &str, len: usize) -> bool {
        known_answer_prefixes(self.brand)
            .any(|prefix| prefix.len() > len && line.starts_with(prefix))
    }
}

/// The text that the answer to a query starts with: the query without its
/// `?` and the space before it.
#[must_use]
pub fn answer_prefix(query: &str) -> &str {
    query.trim_end().trim_end_matches('?').trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker(queries: &[&'static str]) -> AnswerTracker {
        let mut tracker = AnswerTracker::new(Brand::Denon);
        for query in queries {
            tracker.written(Cow::Borrowed(query));
        }
        tracker
    }

    #[test]
    fn a_line_answers_its_query() {
        let mut tracker = tracker(&["MNMEN?", "PSCINEMA EQ. ?"]);
        assert_eq!(tracker.line("PSCINEMA EQ.OFF"), AnswerLine::Answer);
        assert_eq!(tracker.line("MVMAX 98"), AnswerLine::Other);
        assert!(!tracker.is_idle());
        assert_eq!(tracker.line("MNMEN OFF"), AnswerLine::Answer);
        assert!(tracker.is_idle());
    }

    #[test]
    fn the_longest_known_query_wins() {
        let mut tracker = tracker(&["MS?", "PSIMAX ?", "Z2?"]);
        // Lines of other known queries do not answer the short ones.
        for line in ["MSQUICK0", "PSIMAXSWM AUTO", "Z2MUOFF", "Z2CVFL 50"] {
            assert_eq!(tracker.line(line), AnswerLine::Other, "{line}");
        }
        assert_eq!(tracker.line("MSDOLBY ATMOS"), AnswerLine::Answer);
        assert_eq!(tracker.line("PSIMAX AUTO"), AnswerLine::Answer);
        assert_eq!(tracker.line("Z2ON"), AnswerLine::Answer);
        assert!(tracker.is_idle());
    }

    #[test]
    fn the_longest_open_query_wins() {
        let mut tracker = tracker(&["PSDEL ?", "PSDELAY ?"]);
        assert_eq!(tracker.line("PSDELAY 000"), AnswerLine::Answer);
        assert_eq!(tracker.take_open().collect::<Vec<_>>(), ["PSDEL ?"]);
    }

    #[test]
    fn a_list_is_open_until_its_end_line() {
        let mut tracker = tracker(&["OPSMLALL ?", "SSAEQLRS ?", "CV?", "Z2CV?"]);
        // `Z2CV?` has no end line.
        assert_eq!(tracker.line("Z2CVFL 50"), AnswerLine::Answer);
        assert_eq!(tracker.line("OPSMLALL MOV011Stereo"), AnswerLine::Answer);
        assert_eq!(tracker.line("OPSMLALL MOV020LEGEND"), AnswerLine::Answer);
        assert_eq!(tracker.line("CVFL 50"), AnswerLine::Answer);
        assert_eq!(tracker.line("CVEND"), AnswerLine::Answer);
        assert_eq!(tracker.line("SSAEQLRSEND"), AnswerLine::Answer);
        assert!(!tracker.is_idle());
        assert_eq!(tracker.line("OPSMLALL END"), AnswerLine::Answer);
        assert!(tracker.is_idle());
    }

    #[test]
    fn later_lines_of_an_answer_are_related() {
        let mut tracker = tracker(&["TR?"]);
        assert_eq!(tracker.line("TR3 ON"), AnswerLine::Answer);
        assert_eq!(tracker.line("TR1 OFF"), AnswerLine::Related);
        tracker.forget_answered();
        assert_eq!(tracker.line("TR2 ON"), AnswerLine::Other);
    }

    #[test]
    fn a_line_before_the_write_answers_nothing() {
        let mut tracker = tracker(&[]);
        assert_eq!(tracker.line("MNMEN OFF"), AnswerLine::Other);
        tracker.written(Cow::Borrowed("MNMEN?"));
        assert!(!tracker.is_idle());
    }

    #[test]
    fn a_query_written_twice_needs_two_answers() {
        let mut tracker = tracker(&["SI?", "SI?"]);
        assert_eq!(tracker.line("SIGAME"), AnswerLine::Answer);
        assert!(!tracker.is_idle());
        assert_eq!(tracker.line("SIGAME"), AnswerLine::Answer);
        assert!(tracker.is_idle());
    }

    #[test]
    fn an_unknown_query_uses_its_own_text() {
        let mut tracker = tracker(&[]);
        tracker.written(Cow::Owned("SSQSN1 ?".to_owned()));
        assert_eq!(tracker.line("SSQSN1 Movie"), AnswerLine::Answer);
        assert!(tracker.is_idle());
    }

    #[test]
    fn a_query_without_text_is_not_kept() {
        let mut tracker = tracker(&["?", " ?"]);
        assert!(tracker.is_idle());
        assert_eq!(tracker.line("XYZ"), AnswerLine::Other);
    }

    #[test]
    fn open_queries_can_be_read_taken_and_cleared() {
        let mut tracker = tracker(&["PSIMAXSWM ?", "MV?", "OPSMLALL ?"]);
        assert_eq!(tracker.line("MV50"), AnswerLine::Answer);
        assert_eq!(tracker.line("OPSMLALL MOV011Stereo"), AnswerLine::Answer);
        let open = ["PSIMAXSWM ?", "OPSMLALL ?"];
        assert_eq!(tracker.open().collect::<Vec<_>>(), open);
        assert_eq!(tracker.take_open().collect::<Vec<_>>(), open);
        assert!(tracker.is_idle());
        tracker.written(Cow::Borrowed("MU?"));
        tracker.clear();
        assert!(tracker.is_idle());
        assert_eq!(tracker.line("MV51"), AnswerLine::Other);
    }

    #[test]
    fn answer_prefixes() {
        for (query, prefix) in [
            ("MV?", "MV"),
            ("PSTONE CTRL ?", "PSTONE CTRL"),
            ("PSMULTEQ: ?", "PSMULTEQ:"),
            ("NSFRN ?", "NSFRN"),
        ] {
            assert_eq!(answer_prefix(query), prefix);
        }
    }
}
