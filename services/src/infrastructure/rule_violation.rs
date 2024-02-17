use std::fmt;
use std::fmt::Formatter;

#[derive(Debug)]
pub enum RuleViolation {
    SignUpEmailEmpty,
    SignUpUsernameEmpty,
    SignUpPasswordEmpty,
    SignUpPasswordRepeatEmpty,
    SignUpEmailInvalid,
    SignUpAccountWithThisEmailAlreadyExists,
    SignUpUsernameAlreadyTaken,
    SignUpPasswordsDoNotMatch,
    SignUpPasswordRequirementsNotMet,

    LoginUsernameEmpty,
    LoginPasswordEmpty,

    MediaInvalidOriginalLanguage,
    MediaTmdbVoteAverageOutOfBounds,

    MovieNegativeRuntime,

    LogRatingOutOfBounds,

    AppTokenNameAlreadyExists,

    PasswordChangeOldPasswordEmpty,
    PasswordChangeNewPasswordEmpty,
    PasswordChangePasswordRepeatEmpty,
    PasswordChangeOldPasswordIncorrect,
    PasswordChangePasswordsDoNotMatch,
    PasswordChangePasswordRequirementsNotMet,
}

impl fmt::Display for RuleViolation {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}