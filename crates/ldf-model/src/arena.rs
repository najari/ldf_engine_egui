//! 타입이 붙은 ID와 arena. 참조는 문자열이 아니라 인덱스이므로 이름 변경이 O(1)이다.

use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

/// arena 인덱스로 쓰는 ID 타입.
pub trait ArenaId: Copy {
    /// 인덱스에서 ID를 만든다.
    fn from_index(i: usize) -> Self;
    /// ID의 인덱스.
    fn index(self) -> usize;
}

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub u32);

        impl ArenaId for $name {
            fn from_index(i: usize) -> Self {
                Self(u32::try_from(i).unwrap_or(u32::MAX))
            }
            fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id_type!(/// 노드 ID.
    NodeId);
id_type!(/// 시그널 ID.
    SignalId);
id_type!(/// 프레임 ID(LIN ID가 아니라 arena 인덱스).
    FrameId);
id_type!(/// 인코딩 타입 ID.
    EncodingId);
id_type!(/// 스케줄 테이블 ID.
    ScheduleId);

/// 삽입 순서를 보존하는 arena.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    transparent,
    bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>")
)]
pub struct Arena<I, T> {
    items: Vec<T>,
    #[serde(skip)]
    marker: PhantomData<I>,
}

impl<I, T> Default for Arena<I, T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            marker: PhantomData,
        }
    }
}

impl<I: ArenaId, T> Arena<I, T> {
    /// 빈 arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// 항목을 추가하고 ID를 돌려준다.
    pub fn push(&mut self, item: T) -> I {
        let id = I::from_index(self.items.len());
        self.items.push(item);
        id
    }

    /// 항목 수.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 비었는지.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// ID로 항목을 찾는다.
    pub fn get(&self, id: I) -> Option<&T> {
        self.items.get(id.index())
    }

    /// ID로 항목을 가변으로 찾는다.
    pub fn get_mut(&mut self, id: I) -> Option<&mut T> {
        self.items.get_mut(id.index())
    }

    /// `(ID, 항목)` 순회(삽입 순서).
    pub fn iter(&self) -> impl Iterator<Item = (I, &T)> {
        self.items
            .iter()
            .enumerate()
            .map(|(i, t)| (I::from_index(i), t))
    }

    /// `(ID, 가변 항목)` 순회.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (I, &mut T)> {
        self.items
            .iter_mut()
            .enumerate()
            .map(|(i, t)| (I::from_index(i), t))
    }

    /// 항목 슬라이스.
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }
}
