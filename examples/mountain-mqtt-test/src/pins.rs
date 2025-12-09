use ariel_os::hal::peripherals;

#[cfg(context = "nrf52840dk")]
ariel_os::hal::define_peripherals!(Peripherals {
    btn1: P0_11,
    led: P0_13
});

#[cfg(context = "nrf5340dk")]
ariel_os::hal::define_peripherals!(Peripherals {
    btn1: P0_23,
    led: P0_28
});

#[cfg(context = "rp")]
ariel_os::hal::define_peripherals!(Peripherals {
    btn1: PIN_18,
    led: PIN_13
});
