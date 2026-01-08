use ariel_os::hal::peripherals;

#[cfg(context = "nrf52840dk")]
ariel_os::hal::define_peripherals!(Peripherals {
    led: P0_13,
    btn1: P0_11
});

#[cfg(context = "nrf5340dk")]
ariel_os::hal::define_peripherals!(Peripherals {
    led: P0_28,
    btn1: P0_23
});

#[cfg(context = "rp")]
ariel_os::hal::define_peripherals!(Peripherals {
    led: PIN_13,
    btn1: PIN_18
});
