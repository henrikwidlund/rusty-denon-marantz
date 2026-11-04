/// Defines an enum with one code and a label for each variant.
macro_rules! code_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $variant:ident = $code:literal, $label:literal; )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum $name {
            $( #[doc = $label] $variant, )+
        }

        impl $name {
            /// All values.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )+ ];

            /// Code used by the receiver.
            #[must_use]
            pub const fn code(self) -> &'static str {
                match self {
                    $( Self::$variant => $code, )+
                }
            }

            /// Name for display.
            #[must_use]
            pub const fn label(self) -> &'static str {
                match self {
                    $( Self::$variant => $label, )+
                }
            }

            /// Parses a code used by the receiver.
            #[must_use]
            pub fn from_code(code: &str) -> Option<Self> {
                match code {
                    $( $code => Some(Self::$variant), )+
                    _ => None,
                }
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.label())
            }
        }
    };
}

/// Defines an enum with a telnet code, an optional AppCommand code and a label
/// for each variant.
macro_rules! dual_code_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $variant:ident = $telnet:literal, $app:expr, $label:literal; )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum $name {
            $( #[doc = $label] $variant, )+
        }

        impl $name {
            /// All values.
            pub const ALL: &'static [Self] = &[ $( Self::$variant, )+ ];

            /// Code used on telnet.
            #[must_use]
            pub const fn telnet_code(self) -> &'static str {
                match self {
                    $( Self::$variant => $telnet, )+
                }
            }

            /// Code used in AppCommand XML. `None` if AppCommand has no code for this value.
            #[must_use]
            pub const fn appcommand_code(self) -> Option<&'static str> {
                match self {
                    $( Self::$variant => $app, )+
                }
            }

            /// Name for display.
            #[must_use]
            pub const fn label(self) -> &'static str {
                match self {
                    $( Self::$variant => $label, )+
                }
            }

            /// Parses a telnet code.
            #[must_use]
            pub fn from_telnet(code: &str) -> Option<Self> {
                match code {
                    $( $telnet => Some(Self::$variant), )+
                    _ => None,
                }
            }

            /// Parses an AppCommand code.
            #[must_use]
            pub fn from_appcommand(code: &str) -> Option<Self> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|value| value.appcommand_code() == Some(code))
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.label())
            }
        }
    };
}
