//! A\* pathfinding on a grid.
//!
//! # Why a grid, and not a navmesh
//!
//! A navmesh is the right answer for a character on arbitrary geometry and the wrong answer
//! for what this engine's games are: a top-down racer, a platformer, a tile map. A grid is
//! what the level data already is, what the editor can show, and what fits in a few hundred
//! lines that can be tested to the last case. A navmesh would bring a mesh generator, a
//! polygon clipper and a funnel algorithm for a result nobody here needs.
//!
//! # The two bugs this implementation is built around
//!
//! 1. **Corner cutting.** With diagonals allowed, a path can slip between two blocked cells
//!    that touch only at a corner — and a character wider than a point walks through the
//!    wall. [`PathOptions::prevent_corner_cutting`] is on by default, and the test for it
//!    uses exactly that "diagonal between two walls" shape.
//! 2. **Non-determinism.** Two nodes with the same `f` are popped in whatever order the heap
//!    happens to produce, so the same map can give a different (equally short) path on two
//!    runs. The tie-break here is `(f, h, y, x)`, which is total: the same query always
//!    returns the same cells, which is what a replay or a test needs.
//!
//! # Cost
//!
//! The heuristic is octile when diagonals are allowed and Manhattan otherwise — both
//! **admissible** for their movement model, so the first time the goal is popped the path is
//! optimal. (Euclidean would also be admissible; octile is tighter and stays integer.)

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// A cell in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GridPoint {
    /// Column, `0` at the left.
    pub x: usize,
    /// Row, `0` at the top.
    pub y: usize,
}

impl GridPoint {
    /// A cell.
    pub const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }

    /// The straight-line distance in cells, for a caller that wants a rough measure.
    pub fn manhattan(self, other: Self) -> usize {
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y)
    }

    /// The cell as `(x, y)`.
    pub const fn to_tuple(self) -> (usize, usize) {
        (self.x, self.y)
    }

    /// The eight (or four) neighbours of this cell, inside `grid`.
    fn neighbours(self, grid: &NavGrid, diagonals: bool) -> impl Iterator<Item = Self> + '_ {
        let mut out = Vec::with_capacity(8);
        let x = self.x as isize;
        let y = self.y as isize;
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            push_if_inside(grid, &mut out, x + dx, y + dy);
        }
        if diagonals {
            for (dx, dy) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
                push_if_inside(grid, &mut out, x + dx, y + dy);
            }
        }
        out.into_iter()
    }
}

fn push_if_inside(grid: &NavGrid, out: &mut Vec<GridPoint>, x: isize, y: isize) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as usize, y as usize);
    if grid.in_bounds(x, y) {
        out.push(GridPoint::new(x, y));
    }
}

/// How a path may move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathOptions {
    /// Whether the eight neighbours are allowed. `false` gives Manhattan movement.
    pub diagonals: bool,
    /// Whether a diagonal step is refused when it would slip between two blocked cells.
    ///
    /// On by default: a character with any width at all cannot pass through a corner, and a
    /// path that says it can is a path that will get stuck on the wall.
    pub prevent_corner_cutting: bool,
    /// How many nodes may be expanded before giving up.
    ///
    /// A pathfinding call on a big map with no route can expand the whole grid. The cap is
    /// what turns "the game froze for a second" into "no path", which a game can show.
    pub max_expansions: usize,
}

impl Default for PathOptions {
    fn default() -> Self {
        Self {
            diagonals: true,
            prevent_corner_cutting: true,
            // 200×200 cells: more than any map in this repository, and still bounded.
            max_expansions: 40_000,
        }
    }
}

/// A grid of walkable and blocked cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavGrid {
    width: usize,
    height: usize,
    /// Row-major, `y * width + x`.
    blocked: Vec<bool>,
}

impl NavGrid {
    /// An empty, fully walkable grid.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            blocked: vec![false; width * height],
        }
    }

    /// A grid from ASCII rows: `#` (and `X`) are blocked, everything else is walkable.
    ///
    /// Rows shorter than the widest one are padded as walkable, so a map read from a file
    /// with a short last line still loads. This is also how the tests draw their maps.
    pub fn from_rows(rows: &[&str]) -> Self {
        let width = rows
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(0);
        let height = rows.len();
        let mut grid = Self::new(width, height);
        for (y, row) in rows.iter().enumerate() {
            for (x, cell) in row.chars().enumerate() {
                if cell == '#' || cell == 'X' {
                    grid.set_blocked(x, y, true);
                }
            }
        }
        grid
    }

    /// The grid's width in cells.
    pub const fn width(&self) -> usize {
        self.width
    }

    /// The grid's height in cells.
    pub const fn height(&self) -> usize {
        self.height
    }

    /// Whether `(x, y)` is inside the grid.
    pub fn in_bounds(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height
    }

    /// Whether a cell is blocked. A cell outside the grid is blocked, which keeps every
    /// caller from having to check bounds first.
    pub fn is_blocked(&self, x: usize, y: usize) -> bool {
        if !self.in_bounds(x, y) {
            return true;
        }
        self.blocked[y * self.width + x]
    }

    /// Marks a cell blocked or walkable.
    pub fn set_blocked(&mut self, x: usize, y: usize, blocked: bool) {
        if self.in_bounds(x, y) {
            self.blocked[y * self.width + x] = blocked;
        }
    }

    /// Whether a cell can be walked on: inside the grid and not blocked.
    pub fn is_walkable(&self, point: GridPoint) -> bool {
        !self.is_blocked(point.x, point.y)
    }

    /// How many cells are blocked.
    pub fn blocked_count(&self) -> usize {
        self.blocked.iter().filter(|cell| **cell).count()
    }

    /// The shortest path from `start` to `goal`, inclusive, or `None`.
    ///
    /// `start` equal to `goal` gives a one-cell path. A blocked start or goal gives `None`:
    /// a caller asking a question with no answer should hear it, not receive a path that
    /// starts inside a wall.
    pub fn find_path(&self, start: GridPoint, goal: GridPoint) -> Option<Vec<GridPoint>> {
        self.find_path_with(start, goal, PathOptions::default())
    }

    /// [`NavGrid::find_path`] with explicit options.
    pub fn find_path_with(
        &self,
        start: GridPoint,
        goal: GridPoint,
        options: PathOptions,
    ) -> Option<Vec<GridPoint>> {
        if !self.is_walkable(start) || !self.is_walkable(goal) {
            return None;
        }
        if start == goal {
            return Some(vec![start]);
        }

        // `f = g + h` is stored as `(f, h)` so the tie-break prefers the node closer to the
        // goal; the point itself is the last key, so the order is total.
        let mut open: BinaryHeap<Reverse<(usize, usize, usize, usize)>> = BinaryHeap::new();
        let mut cost: HashMap<GridPoint, usize> = HashMap::new();
        let mut came_from: HashMap<GridPoint, GridPoint> = HashMap::new();
        let mut expanded = 0;

        cost.insert(start, 0);
        open.push(Reverse((
            self.heuristic(start, goal, options),
            self.heuristic(start, goal, options),
            start.y,
            start.x,
        )));

        while let Some(Reverse((_, _, y, x))) = open.pop() {
            let current = GridPoint::new(x, y);
            if current == goal {
                return Some(reconstruct(&came_from, start, goal));
            }
            let Some(&current_cost) = cost.get(&current) else {
                continue;
            };
            expanded += 1;
            if expanded > options.max_expansions {
                return None;
            }

            for next in current.neighbours(self, options.diagonals) {
                if !self.is_walkable(next) {
                    continue;
                }
                let diagonal = next.x != current.x && next.y != current.y;
                if diagonal && options.prevent_corner_cutting {
                    // Both orthogonal cells around the corner have to be free, or the
                    // character clips through the join.
                    if self.is_blocked(next.x, current.y) || self.is_blocked(current.x, next.y) {
                        continue;
                    }
                }
                let step = if diagonal { 14 } else { 10 };
                let candidate = current_cost + step;
                if cost.get(&next).is_some_and(|known| *known <= candidate) {
                    continue;
                }
                cost.insert(next, candidate);
                came_from.insert(next, current);
                let heuristic = self.heuristic(next, goal, options);
                open.push(Reverse((candidate + heuristic, heuristic, next.y, next.x)));
            }
        }
        None
    }

    /// The heuristic cost from `from` to `to`: octile with diagonals, Manhattan without.
    ///
    /// Both are scaled by 10 to match the step costs, so the arithmetic stays integer.
    fn heuristic(&self, from: GridPoint, to: GridPoint, options: PathOptions) -> usize {
        let dx = from.x.abs_diff(to.x);
        let dy = from.y.abs_diff(to.y);
        if options.diagonals {
            // Octile: `14 * min + 10 * (max - min)` is the true cost of the diagonal path.
            let (low, high) = if dx < dy { (dx, dy) } else { (dy, dx) };
            14 * low + 10 * (high - low)
        } else {
            10 * (dx + dy)
        }
    }

    /// Whether a straight line of cells between `from` and `to` is walkable.
    ///
    /// Bresenham, plus the corner rule: a step that moves diagonally across the join of two
    /// blocked cells is not clear. Used by [`NavGrid::smooth`], which is why it has to agree
    /// with the A\* corner rule — otherwise smoothing would hand back a path A\* refused.
    pub fn line_of_sight(&self, from: GridPoint, to: GridPoint) -> bool {
        let (mut x, mut y) = (from.x as isize, from.y as isize);
        let (target_x, target_y) = (to.x as isize, to.y as isize);
        let dx = (target_x - x).abs();
        let dy = -(target_y - y).abs();
        let step_x = if x < target_x { 1 } else { -1 };
        let step_y = if y < target_y { 1 } else { -1 };
        let mut error = dx + dy;

        loop {
            if self.is_blocked(x as usize, y as usize) {
                return false;
            }
            if x == target_x && y == target_y {
                return true;
            }
            let previous = (x, y);
            let doubled = 2 * error;
            if doubled >= dy {
                error += dy;
                x += step_x;
            }
            if doubled <= dx {
                error += dx;
                y += step_y;
            }
            // A diagonal move needs both orthogonal cells free, exactly as in `find_path`.
            if x != previous.0
                && y != previous.1
                && (self.is_blocked(x as usize, previous.1 as usize)
                    || self.is_blocked(previous.0 as usize, y as usize))
            {
                return false;
            }
        }
    }

    /// Reduces `path` to the waypoints a straight line can connect.
    ///
    /// The cells A\* returns hug the corners of every wall. Following them literally makes a
    /// character stutter along each step; following the smoothed waypoints in a straight line
    /// between them is what looks like walking.
    pub fn smooth(&self, path: &[GridPoint]) -> Vec<GridPoint> {
        if path.len() <= 2 {
            return path.to_vec();
        }
        let mut waypoints = vec![path[0]];
        let mut anchor = 0;
        while anchor < path.len() - 1 {
            let mut furthest = anchor + 1;
            for candidate in (anchor + 1)..path.len() {
                if self.line_of_sight(path[anchor], path[candidate]) {
                    furthest = candidate;
                } else {
                    break;
                }
            }
            waypoints.push(path[furthest]);
            anchor = furthest;
        }
        waypoints
    }
}

/// Walks the parent links back from `goal` to `start`.
fn reconstruct(
    came_from: &HashMap<GridPoint, GridPoint>,
    start: GridPoint,
    goal: GridPoint,
) -> Vec<GridPoint> {
    let mut path = vec![goal];
    let mut current = goal;
    while current != start {
        let Some(parent) = came_from.get(&current) else {
            break;
        };
        path.push(*parent);
        current = *parent;
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_straight_line_on_an_empty_grid() {
        let grid = NavGrid::new(10, 10);
        let path = grid
            .find_path(GridPoint::new(0, 0), GridPoint::new(5, 0))
            .expect("path");
        assert_eq!(path.first(), Some(&GridPoint::new(0, 0)));
        assert_eq!(path.last(), Some(&GridPoint::new(5, 0)));
        // Five steps, six cells, no diagonal detour.
        assert_eq!(path.len(), 6);
    }

    #[test]
    fn a_start_equal_to_the_goal_is_a_path_of_one() {
        let grid = NavGrid::new(4, 4);
        let path = grid
            .find_path(GridPoint::new(2, 2), GridPoint::new(2, 2))
            .expect("path");
        assert_eq!(path, vec![GridPoint::new(2, 2)]);
    }

    #[test]
    fn a_wall_forces_a_detour_and_the_path_goes_around_it() {
        // A vertical wall with one gap at the bottom.
        let grid = NavGrid::from_rows(&[
            "..#..", //
            "..#..", //
            "..#..", //
            "..#..", //
            ".....", //
        ]);
        let path = grid
            .find_path(GridPoint::new(0, 0), GridPoint::new(4, 0))
            .expect("path");
        assert_eq!(path.first(), Some(&GridPoint::new(0, 0)));
        assert_eq!(path.last(), Some(&GridPoint::new(4, 0)));
        // Every cell is walkable, and the route used the gap (row 4).
        assert!(path.iter().all(|point| grid.is_walkable(*point)));
        assert!(path.iter().any(|point| point.y == 4), "{path:?}");
    }

    #[test]
    fn a_goal_walled_in_has_no_path() {
        let grid = NavGrid::from_rows(&[
            "..#..", //
            ".#.#.", //
            "..#..", //
        ]);
        assert!(grid
            .find_path(GridPoint::new(0, 0), GridPoint::new(2, 1))
            .is_none());
    }

    #[test]
    fn a_blocked_start_or_goal_is_refused() {
        let grid = NavGrid::from_rows(&["#..", "...", "..#"]);
        assert!(grid
            .find_path(GridPoint::new(0, 0), GridPoint::new(1, 1))
            .is_none());
        assert!(grid
            .find_path(GridPoint::new(1, 1), GridPoint::new(2, 2))
            .is_none());
    }

    #[test]
    fn a_cell_outside_the_grid_counts_as_blocked() {
        let grid = NavGrid::new(3, 3);
        assert!(grid.is_blocked(3, 0));
        assert!(grid.is_blocked(0, 3));
        assert!(!grid.is_blocked(2, 2));
    }

    #[test]
    fn diagonals_are_used_when_allowed_and_not_when_refused() {
        let grid = NavGrid::new(8, 8);
        let with = grid
            .find_path_with(
                GridPoint::new(0, 0),
                GridPoint::new(3, 3),
                PathOptions::default(),
            )
            .expect("path");
        // Three diagonal steps: four cells.
        assert_eq!(with.len(), 4, "{with:?}");

        let without = grid
            .find_path_with(
                GridPoint::new(0, 0),
                GridPoint::new(3, 3),
                PathOptions {
                    diagonals: false,
                    ..PathOptions::default()
                },
            )
            .expect("path");
        // Six orthogonal steps: seven cells.
        assert_eq!(without.len(), 7, "{without:?}");
    }

    #[test]
    fn a_diagonal_between_two_walls_is_refused() {
        // The classic shape: the only way from (0,0) to (1,1) is the corner between the two
        // blocked cells. With corner cutting prevented there is no path at all.
        let grid = NavGrid::from_rows(&[
            ".#", //
            "#.", //
        ]);
        assert!(
            grid.find_path_with(
                GridPoint::new(0, 0),
                GridPoint::new(1, 1),
                PathOptions::default(),
            )
            .is_none(),
            "corner cutting must be refused"
        );
        // Allowing it gives the two-cell path, which is what the option is for.
        let path = grid
            .find_path_with(
                GridPoint::new(0, 0),
                GridPoint::new(1, 1),
                PathOptions {
                    prevent_corner_cutting: false,
                    ..PathOptions::default()
                },
            )
            .expect("path");
        assert_eq!(path.len(), 2);
    }

    #[test]
    fn the_same_query_always_returns_the_same_path() {
        // Two equal-cost routes around a block: the tie-break has to pick one, always.
        let grid = NavGrid::from_rows(&[
            ".....", //
            "..#..", //
            ".....", //
        ]);
        let first = grid
            .find_path(GridPoint::new(0, 1), GridPoint::new(4, 1))
            .expect("path");
        for _ in 0..8 {
            assert_eq!(
                grid.find_path(GridPoint::new(0, 1), GridPoint::new(4, 1))
                    .expect("path"),
                first
            );
        }
    }

    #[test]
    fn the_expansion_cap_turns_a_hopeless_search_into_no_path() {
        let grid = NavGrid::new(200, 200);
        let options = PathOptions {
            max_expansions: 4,
            ..PathOptions::default()
        };
        assert!(grid
            .find_path_with(GridPoint::new(0, 0), GridPoint::new(199, 199), options)
            .is_none());
    }

    #[test]
    fn smoothing_reduces_a_path_to_its_corners_without_moving_the_ends() {
        let grid = NavGrid::from_rows(&[
            ".....", //
            ".....", //
            "..#..", //
            ".....", //
            ".....", //
        ]);
        let path = grid
            .find_path(GridPoint::new(0, 2), GridPoint::new(4, 2))
            .expect("path");
        let smoothed = grid.smooth(&path);
        assert_eq!(smoothed.first(), path.first());
        assert_eq!(smoothed.last(), path.last());
        assert!(smoothed.len() <= path.len());
        // Every smoothed leg is clear, which is the property that makes it usable.
        for pair in smoothed.windows(2) {
            assert!(grid.line_of_sight(pair[0], pair[1]), "{pair:?}");
        }
    }

    #[test]
    fn line_of_sight_is_blocked_by_a_wall_and_clear_otherwise() {
        let grid = NavGrid::from_rows(&["...", ".#.", "..."]);
        // Around the block, not through it.
        assert!(grid.line_of_sight(GridPoint::new(0, 0), GridPoint::new(2, 0)));
        assert!(grid.line_of_sight(GridPoint::new(0, 1), GridPoint::new(0, 2)));
        assert!(grid.line_of_sight(GridPoint::new(2, 0), GridPoint::new(2, 2)));
        // The diagonal from a corner to the opposite corner passes through the block.
        assert!(!grid.line_of_sight(GridPoint::new(0, 0), GridPoint::new(2, 2)));
        // Straight into it, from either side.
        assert!(!grid.line_of_sight(GridPoint::new(0, 1), GridPoint::new(2, 1)));
        assert!(!grid.line_of_sight(GridPoint::new(1, 0), GridPoint::new(1, 2)));
    }

    #[test]
    fn a_diagonal_line_through_a_corner_between_two_walls_is_not_clear() {
        // The same rule A* uses: (0,0) -> (1,1) crosses the join of two blocked cells.
        let grid = NavGrid::from_rows(&[".#", "#."]);
        assert!(!grid.line_of_sight(GridPoint::new(0, 0), GridPoint::new(1, 1)));
    }

    #[test]
    fn smoothing_a_short_path_returns_it_unchanged() {
        let grid = NavGrid::new(4, 4);
        assert_eq!(grid.smooth(&[]), Vec::new());
        let one = vec![GridPoint::new(0, 0)];
        assert_eq!(grid.smooth(&one), one);
        let two = vec![GridPoint::new(0, 0), GridPoint::new(1, 0)];
        assert_eq!(grid.smooth(&two), two);
    }

    #[test]
    fn a_path_never_repeats_a_cell() {
        let grid = NavGrid::from_rows(&[
            "##########", //
            "#........#", //
            "#.######.#", //
            "#.#....#.#", //
            "#.#.##.#.#", //
            "#.#.#..#.#", //
            "#.#.####.#", //
            "#.#......#", //
            "#.########", //
            "#........#", //
            "##########", //
        ]);
        let path = grid
            .find_path(GridPoint::new(1, 9), GridPoint::new(8, 6))
            .expect("path");
        let mut unique = path.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), path.len(), "a cell was visited twice");
        for pair in path.windows(2) {
            let dx = pair[0].x.abs_diff(pair[1].x);
            let dy = pair[0].y.abs_diff(pair[1].y);
            assert!(dx <= 1 && dy <= 1 && (dx + dy) > 0, "{pair:?}");
        }
    }

    #[test]
    fn from_rows_pads_short_lines_and_reports_its_size() {
        let grid = NavGrid::from_rows(&["###", "#", "#####"]);
        assert_eq!(grid.width(), 5);
        assert_eq!(grid.height(), 3);
        assert_eq!(grid.blocked_count(), 3 + 1 + 5);
        assert!(!grid.is_blocked(3, 1), "the missing cells are walkable");
    }
}
