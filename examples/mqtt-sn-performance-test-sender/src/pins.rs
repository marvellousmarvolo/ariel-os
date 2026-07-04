use ariel_os::hal::peripherals;

#[cfg(context = "nrf5340dk-app")]
ariel_os::hal::define_peripherals!(Peripherals { signal_pin: P1_05 });
