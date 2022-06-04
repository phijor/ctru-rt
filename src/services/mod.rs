// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub mod ac;
pub mod apt;
pub mod cfg;
pub mod gsp;
pub mod hid;
pub mod soc;

pub trait Service: Default {}
