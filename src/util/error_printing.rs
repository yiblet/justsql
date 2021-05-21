use std::fmt::{Debug, Write};
use thiserror::Error;

pub trait PrintableError {
    fn print_error<W: Write>(&self, writer: &mut W) -> Result<(), PrintError>;
}

impl<'a, T: PrintableError> PrintableError for &'a [T] {
    fn print_error<W: Write>(&self, writer: &mut W) -> Result<(), PrintError> {
        for v in self.iter() {
            v.print_error(writer)?;
            writer.write_str("\n")?;
        }
        Ok(())
    }
}

impl<'a, T: PrintableError> PrintableError for &'a Vec<T> {
    fn print_error<W: Write>(&self, writer: &mut W) -> Result<(), PrintError> {
        self.as_slice().print_error(writer)
    }
}

#[derive(Error, Debug)]
pub enum PrintError {
    #[error(transparent)]
    FormatError(#[from] std::fmt::Error),
    #[error("could not find error position")]
    MissingPositionError,
    #[error("could not find line")]
    MissingLineError,
}

fn line_pad<W: Write>(writer: &mut W, row: usize, include_line: bool) -> Result<(), PrintError> {
    let positions = (row as f64).log10().floor() as usize + 1;
    if include_line {
        write!(writer, "{: >width$} |", row, width = positions)?;
    } else {
        write!(writer, "{: >width$} |", "", width = positions)?;
    }
    Ok(())
}

fn file_name_pad<W: Write>(writer: &mut W, row: usize) -> Result<(), PrintError> {
    let positions = (row as f64).log10().floor() as usize + 1;
    write!(writer, "{: >width$}{}", "", "-->", width = positions)?;
    Ok(())
}

pub fn print_unpositioned_error<W: Write>(
    writer: &mut W,
    explanation: &str,
    file_name: &str,
) -> Result<(), PrintError> {
    file_name_pad(writer, 1)?;
    write!(writer, " {}\n", file_name)?;

    line_pad(writer, 1, false)?;
    write!(writer, " {}\n", explanation)?;
    Ok(())
}

struct Position {
    row: usize,
    col: usize,
    idx: usize,
}

/// find the row and column of a file given a character position
fn find_row_col(file: &str, position: usize) -> Result<Position, PrintError> {
    let iter = file
        .char_indices()
        .scan((1usize, 0usize), |pos, (idx, chr)| {
            if chr == '\n' {
                pos.0 += 1;
                pos.1 = idx;
            };
            Some((pos.0, idx - pos.1, idx))
        });

    let mut prev = None;
    let mut data = None;
    for (row, col, idx) in iter {
        prev = data.take();
        data = Some((row, col, idx));
        if idx == position {
            break;
        }
    }

    // NOTE this fixes a bug where we get MissingPositionError and
    // MissingLineErrors if an error occurs at EOF
    // if we're at a new line then point to the character of
    // the previous line
    if matches!(data, Some((_, 0, _))) && prev.is_some() {
        data = prev
    }

    data.map(|(row, col, idx)| Position { row, col, idx })
        .ok_or_else(|| PrintError::MissingPositionError)
}

pub fn print_error<W: Write>(
    writer: &mut W,
    file: &str,
    position: usize,
    explanation: &str,
    file_name: &str,
) -> Result<(), PrintError> {
    debug!(
        "finding error in file {} at position {}",
        file_name, position
    );
    if file.len() == 0 {
        return print_unpositioned_error(
            writer,
            format!("empty file: {}", explanation).as_str(),
            file_name,
        );
    }

    // we shadow position here since find_row_col will sometimes
    // go back a line if the current position is at the line beginning.
    let Position {
        row,
        col,
        idx: position,
    } = find_row_col(file, position)?;

    let line = file
        .get(position - col + 1..)
        .ok_or_else(|| PrintError::MissingLineError)?;
    let line = &line[0..line.find('\n').unwrap_or(line.len())];

    file_name_pad(writer, row)?;
    write!(writer, " {}:{}:{}\n", file_name, row, col)?;

    line_pad(writer, row, false)?;
    write!(writer, "\n")?;

    line_pad(writer, row, true)?;
    write!(writer, " {}\n", line)?;

    line_pad(writer, row, false)?;
    write!(writer, "{:col$}^{}\n", "", explanation, col = col)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_pad_test() {
        let mut res = String::new();
        line_pad(&mut res, 22332323, true).unwrap();
        assert_eq!("22332323 |", res);

        let mut res = String::new();
        line_pad(&mut res, 22332323, false).unwrap();
        assert_eq!("         |", res);

        let mut res = String::new();
        file_name_pad(&mut res, 22332323).unwrap();
        assert_eq!("        -->", res);
    }

    #[test]
    fn unpositioned_test() {
        let mut res = String::new();
        print_unpositioned_error(&mut res, "could not read file", "src/test.sql").unwrap();
        assert_eq!(
            format!("\n{}", res.as_str()),
            r#"
 --> src/test.sql
  | could not read file
"#
        )
    }

    #[test]
    fn print_error_test() {
        let example_string = r#"
select * from users 
where userId = @userId
and email = @email
limit 1
"#;
        let file_name = "src/text.sql";

        let mut res = String::new();
        print_error(&mut res, example_string, 28, "unexpected token", file_name).unwrap();
        assert_eq!(&example_string[28..28 + 6], "userId");
        assert_eq!(
            format!("\n{}", res.as_str()),
            r#"
 --> src/text.sql:3:7
  |
3 | where userId = @userId
  |       ^unexpected token
"#
        );

        let mut res = String::new();
        print_error(&mut res, example_string, 21, "unexpected token", file_name).unwrap();
        assert_eq!(&example_string[21..21 + 6], "\nwhere");
        assert_eq!(
            format!("\n{}", res.as_str()),
            r#"
 --> src/text.sql:2:20
  |
2 | select * from users 
  |                    ^unexpected token
"#
        )
    }

    #[test]
    fn row_position_test() {
        fn assert_row_position(data: &str) {
            let mut prev;
            let mut cur = None;
            for (idx, _) in data.char_indices() {
                prev = cur.take();
                cur = find_row_col(data, idx).ok().map(|pos| (pos.row, pos.col));
                assert!(cur.is_some());
                if prev.is_some() {
                    assert!(prev <= cur)
                }
            }
        }

        assert_row_position(
            r#"-- @endpoint update_data
-- @param test
select * from users
where id = user.id
"#,
        );

        assert_row_position(r#"--"#);

        assert_row_position(
            r#"select * from users
testing 123"#,
        );
    }
}
