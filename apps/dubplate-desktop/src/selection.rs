//! Which rows of a list are selected, as rekordbox's track list has it: one focused row that
//! the keys and Load act on, and the others a Command-click, a Shift-click or Command+A added.

use std::collections::HashSet;
use std::hash::Hash;

/// The selected rows: the focused one and the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection<Row: Copy + Eq + Hash> {
    focused: Option<Row>,
    anchor: Option<Row>,
    members: HashSet<Row>,
}

impl<Row: Copy + Eq + Hash> Default for Selection<Row> {
    fn default() -> Self {
        Self {
            focused: None,
            anchor: None,
            members: HashSet::new(),
        }
    }
}

impl<Row: Copy + Eq + Hash> Selection<Row> {
    /// The focused row, which Load and the cursor use.
    pub fn focused(&self) -> Option<Row> {
        self.focused
    }

    /// Whether `row` is selected.
    pub fn contains(&self, row: Row) -> bool {
        self.members.contains(&row)
    }

    /// How many rows are selected.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.members.len()
    }

    /// Selects `row` alone and focuses it.
    pub fn select_only(&mut self, row: Option<Row>) {
        self.focused = row;
        self.anchor = row;
        self.members = row.into_iter().collect();
    }

    /// Moves the focus to the selected row `row`, keeping the selection.
    pub fn focus(&mut self, row: Row) {
        if self.members.contains(&row) {
            self.focused = Some(row);
        }
    }

    /// Command-click: adds `row` and focuses it, or takes it out when it is selected and
    /// another row stays selected.
    pub fn toggle(&mut self, row: Row, rows: &[Row]) {
        if self.members.contains(&row) {
            if self.members.len() == 1 {
                return;
            }
            self.members.remove(&row);
            if self.focused == Some(row) {
                self.focused = rows
                    .iter()
                    .copied()
                    .find(|other| self.members.contains(other));
            }
            self.anchor = self.focused;
        } else {
            self.members.insert(row);
            self.focused = Some(row);
            self.anchor = Some(row);
        }
    }

    /// Shift-click: selects every row from the anchor to `row`, and focuses `row`.
    pub fn select_range(&mut self, row: Row, rows: &[Row]) {
        let anchor = self.anchor.or(self.focused).unwrap_or(row);
        let (Some(from), Some(to)) = (
            rows.iter().position(|other| *other == anchor),
            rows.iter().position(|other| *other == row),
        ) else {
            self.select_only(Some(row));
            return;
        };
        let (first, last) = (from.min(to), from.max(to));
        self.members = rows[first..=last].iter().copied().collect();
        self.focused = Some(row);
        self.anchor = Some(anchor);
    }

    /// Command+A: selects every row, keeping the focus when it is among them.
    pub fn select_all(&mut self, rows: &[Row]) {
        self.members = rows.iter().copied().collect();
        if self.focused.is_none_or(|row| !self.members.contains(&row)) {
            self.focused = rows.first().copied();
        }
        self.anchor = self.focused;
    }

    /// Shift plus an arrow: moves the focus to `row` and selects from the anchor to it.
    pub fn extend_to(&mut self, row: Row, rows: &[Row]) {
        self.select_range(row, rows);
    }

    /// Drops the rows `rows` no longer holds, and refocuses on the first row when the focused
    /// one is gone.
    pub fn retain(&mut self, rows: &[Row]) {
        let kept: HashSet<Row> = rows.iter().copied().collect();
        self.members.retain(|row| kept.contains(row));
        if self.focused.is_none_or(|row| !kept.contains(&row)) {
            self.select_only(rows.first().copied());
        } else if self.anchor.is_none_or(|row| !kept.contains(&row)) {
            self.anchor = self.focused;
        }
    }

    /// Whether the row's context menu targets any row matching `predicate`.
    pub fn any_in_context_menu(&self, row: Row, mut predicate: impl FnMut(Row) -> bool) -> bool {
        if self.contains(row) {
            self.members.iter().copied().any(predicate)
        } else {
            predicate(row)
        }
    }

    /// The selected rows in the order `rows` lists them.
    pub fn in_order(&self, rows: &[Row]) -> Vec<Row> {
        rows.iter()
            .copied()
            .filter(|row| self.members.contains(row))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS: [u32; 6] = [10, 11, 12, 13, 14, 15];

    fn selected(selection: &Selection<u32>) -> Vec<u32> {
        selection.in_order(&ROWS)
    }

    #[test]
    fn command_click_adds_and_removes_rows() {
        let mut selection = Selection::default();
        selection.select_only(Some(10));
        selection.toggle(12, &ROWS);
        assert_eq!(selected(&selection), [10, 12]);
        assert_eq!(selection.focused(), Some(12));
        selection.toggle(12, &ROWS);
        assert_eq!(selected(&selection), [10]);
        assert_eq!(selection.focused(), Some(10));
        selection.toggle(10, &ROWS);
        assert_eq!(selected(&selection), [10], "the last row stays selected");
    }

    #[test]
    fn shift_click_selects_a_range_from_the_anchor_either_way() {
        let mut selection = Selection::default();
        selection.select_only(Some(11));
        selection.select_range(14, &ROWS);
        assert_eq!(selected(&selection), [11, 12, 13, 14]);
        assert_eq!(selection.focused(), Some(14));
        selection.select_range(10, &ROWS);
        assert_eq!(selected(&selection), [10, 11]);
        assert_eq!(selection.focused(), Some(10));
    }

    #[test]
    fn select_all_keeps_the_focus() {
        let mut selection = Selection::default();
        selection.select_only(Some(13));
        selection.select_all(&ROWS);
        assert_eq!(selection.len(), 6);
        assert_eq!(selection.focused(), Some(13));
    }

    #[test]
    fn shift_and_an_arrow_extend_then_shrink_the_selection() {
        let mut selection = Selection::default();
        selection.select_only(Some(11));
        selection.extend_to(12, &ROWS);
        selection.extend_to(13, &ROWS);
        assert_eq!(selected(&selection), [11, 12, 13]);
        selection.extend_to(12, &ROWS);
        assert_eq!(selected(&selection), [11, 12]);
    }

    #[test]
    fn rows_that_leave_the_list_leave_the_selection() {
        let mut selection = Selection::default();
        selection.select_only(Some(10));
        selection.select_range(12, &ROWS);
        selection.retain(&[11, 12, 13]);
        assert_eq!(selection.in_order(&[11, 12, 13]), [11, 12]);
        assert_eq!(selection.focused(), Some(12));
        selection.retain(&[13, 14]);
        assert_eq!(selection.focused(), Some(13));
        assert_eq!(selection.len(), 1);
    }
}
