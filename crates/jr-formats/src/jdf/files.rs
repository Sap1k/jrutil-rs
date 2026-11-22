//! JDF-R017, R019: the files of a batch and where they are in a container.

use super::JdfError;

/// The files of a batch, in the order of [`File::ALL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum File {
    Verze,
    Zastavky,
    Oznacniky,
    Dopravci,
    Linky,
    LinExt,
    Zaslinky,
    Spoje,
    SpojSkup,
    Zasspoje,
    Udaje,
    Pevnykod,
    Caskody,
    Navaznosti,
    Altdop,
    Altlinky,
    Mistenky,
}

impl File {
    pub(super) const ALL: [File; 17] = [
        File::Verze,
        File::Zastavky,
        File::Oznacniky,
        File::Dopravci,
        File::Linky,
        File::LinExt,
        File::Zaslinky,
        File::Spoje,
        File::SpojSkup,
        File::Zasspoje,
        File::Udaje,
        File::Pevnykod,
        File::Caskody,
        File::Navaznosti,
        File::Altdop,
        File::Altlinky,
        File::Mistenky,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            File::Verze => "VerzeJDF.txt",
            File::Zastavky => "Zastavky.txt",
            File::Oznacniky => "Oznacniky.txt",
            File::Dopravci => "Dopravci.txt",
            File::Linky => "Linky.txt",
            File::LinExt => "LinExt.txt",
            File::Zaslinky => "Zaslinky.txt",
            File::Spoje => "Spoje.txt",
            File::SpojSkup => "SpojSkup.txt",
            File::Zasspoje => "Zasspoje.txt",
            File::Udaje => "Udaje.txt",
            File::Pevnykod => "Pevnykod.txt",
            File::Caskody => "Caskody.txt",
            File::Navaznosti => "Navaznosti.txt",
            File::Altdop => "Altdop.txt",
            File::Altlinky => "Altlinky.txt",
            File::Mistenky => "Mistenky.txt",
        }
    }

    /// A batch of any version must have it.
    pub(super) fn required(self) -> bool {
        matches!(
            self,
            File::Verze
                | File::Zastavky
                | File::Dopravci
                | File::Linky
                | File::Zaslinky
                | File::Spoje
                | File::Zasspoje
                | File::Pevnykod
                | File::Caskody
        )
    }

    /// The file exists from JDF 1.10 on; a 1.8/1.9 batch's copy is not read.
    pub(super) fn since_1_10(self) -> bool {
        matches!(
            self,
            File::Oznacniky | File::LinExt | File::SpojSkup | File::Navaznosti
        )
    }

    fn of(file_name: &str) -> Option<File> {
        File::ALL
            .into_iter()
            .find(|file| file.name().eq_ignore_ascii_case(file_name))
    }
}

/// For each of [`File::ALL`], the index of its entry among `names`.
///
/// Names are paths with `/` or `\` separators; entries ending in a separator are directories.
/// The batch root is the directory holding `VerzeJDF.txt`; there must be exactly one. Files are
/// matched by name in that directory, ignoring ASCII case; two entries for one file are an error.
/// Other entries are ignored.
pub(super) fn locate<'n>(
    names: impl IntoIterator<Item = &'n str>,
) -> Result<[Option<usize>; 17], JdfError> {
    let entries: Vec<(&str, &str)> = names
        .into_iter()
        .map(|name| match name.rfind(['/', '\\']) {
            Some(at) => (&name[..at], &name[at + 1..]),
            None => ("", name),
        })
        .collect();
    let mut roots: Vec<&str> = entries
        .iter()
        .filter(|(_, file)| file.eq_ignore_ascii_case(File::Verze.name()))
        .map(|&(dir, _)| dir)
        .collect();
    roots.sort_unstable();
    roots.dedup();
    let root = match roots.as_slice() {
        [] => return Err(JdfError::NoVersionFile),
        [root] => *root,
        _ => {
            return Err(JdfError::SeveralBatches(
                roots.iter().map(|&root| Box::from(root)).collect(),
            ));
        }
    };
    let mut found = [None; 17];
    for (index, &(dir, name)) in entries.iter().enumerate() {
        if dir != root {
            continue;
        }
        let Some(file) = File::of(name) else {
            continue;
        };
        let slot = &mut found[file as usize];
        if slot.is_some() {
            return Err(JdfError::DuplicateFile(file.name()));
        }
        *slot = Some(index);
    }
    Ok(found)
}
