use glib::subclass::prelude::*;

use crate::subclass::prelude::ActionRowImpl;
use crate::SpinRow;

pub trait SpinRowImpl: ActionRowImpl {}

unsafe impl<T: SpinRowImpl> IsSubclassable<T> for SpinRow {}
