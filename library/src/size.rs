//! Encoded size guarantees and checked composition.

/// The number of bytes consumed by a successful encode or decode operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodedSize {
    /// Every successful representation has exactly this size.
    Fixed(usize),
    /// Every successful representation fits within this upper bound.
    Bounded(usize),
    /// No finite upper bound is promised.
    Unbounded,
}

impl EncodedSize {
    /// Merge alternatives. Empty alternatives are a declaration error.
    /// Equal fixed branches remain fixed; other finite branches use their maximum.
    pub const fn choice(parts: &[Self]) -> Self {
        assert!(
            !parts.is_empty(),
            "encoded choice requires at least one branch"
        );
        let first = match parts[0] {
            Self::Fixed(n) | Self::Bounded(n) => n,
            Self::Unbounded => return Self::Unbounded,
        };
        let mut maximum = first;
        let mut fixed = matches!(parts[0], Self::Fixed(_));
        let mut index = 1;
        while index < parts.len() {
            let n = match parts[index] {
                Self::Fixed(n) => {
                    if n != first {
                        fixed = false;
                    }
                    n
                }
                Self::Bounded(n) => {
                    fixed = false;
                    n
                }
                Self::Unbounded => return Self::Unbounded,
            };
            if n > maximum {
                maximum = n;
            }
            index += 1;
        }
        if fixed {
            Self::Fixed(maximum)
        } else {
            Self::Bounded(maximum)
        }
    }

    pub const fn maximum(self) -> Option<usize> {
        match self {
            Self::Fixed(n) | Self::Bounded(n) => Some(n),
            Self::Unbounded => None,
        }
    }

    pub const fn accepts(self, used: usize) -> bool {
        match self {
            Self::Fixed(n) => used == n,
            Self::Bounded(n) => used <= n,
            Self::Unbounded => true,
        }
    }

    /// Restrict an existing guarantee. An impossible fixed size is a declaration error.
    pub const fn limited(self, limit: usize) -> Self {
        match self {
            Self::Fixed(n) => {
                assert!(n <= limit, "fixed encoding exceeds frame maximum");
                Self::Fixed(n)
            }
            Self::Bounded(n) => Self::Bounded(if n < limit { n } else { limit }),
            Self::Unbounded => Self::Bounded(limit),
        }
    }

    /// Compose sequential values. Overflow is a declaration error.
    pub const fn sum(parts: &[Self]) -> Self {
        let mut total = 0usize;
        let mut fixed = true;
        let mut unbounded = false;
        let mut index = 0;
        while index < parts.len() {
            match parts[index] {
                Self::Fixed(n) => total = checked_add(total, n),
                Self::Bounded(n) => {
                    total = checked_add(total, n);
                    fixed = false;
                }
                Self::Unbounded => unbounded = true,
            }
            index += 1;
        }
        if unbounded {
            Self::Unbounded
        } else if fixed {
            Self::Fixed(total)
        } else {
            Self::Bounded(total)
        }
    }

    /// Exactly `count` elements, as used by an array.
    pub const fn repeated(self, count: usize) -> Self {
        if count == 0 {
            return Self::Fixed(0);
        }
        match self {
            Self::Fixed(n) => Self::Fixed(checked_mul(n, count)),
            Self::Bounded(n) => Self::Bounded(checked_mul(n, count)),
            Self::Unbounded => Self::Unbounded,
        }
    }

    /// Between zero and `count` elements, as used by a counted sequence.
    pub const fn up_to(self, count: usize) -> Self {
        match self.repeated(count) {
            Self::Fixed(0) => Self::Fixed(0),
            Self::Fixed(n) | Self::Bounded(n) => Self::Bounded(n),
            Self::Unbounded => Self::Unbounded,
        }
    }
}

const fn checked_add(left: usize, right: usize) -> usize {
    match left.checked_add(right) {
        Some(value) => value,
        None => panic!("encoded size addition overflow"),
    }
}

const fn checked_mul(left: usize, right: usize) -> usize {
    match left.checked_mul(right) {
        Some(value) => value,
        None => panic!("encoded size multiplication overflow"),
    }
}
