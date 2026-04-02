use crate::BreakpointBin;
use glib::subclass::prelude::*;
use gtk::subclass::prelude::WidgetImpl;

pub trait BreakpointBinImpl: WidgetImpl {}

unsafe impl<T: BreakpointBinImpl> IsSubclassable<T> for BreakpointBin {}
