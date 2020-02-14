pub enum NumericReply {
    RplWelcome = 1,
    RplYourHost = 2,
    RplCreated = 3,
    RplYoureOper = 381,
    ErrNoSuchNick = 401,
    ErrNoSuchChannel = 403,
    ErrCannotSendToChan = 404,
    ErrNoRecipient = 411,
    ErrNoTextToSend = 412,
    ErrNoNicknameGiven = 431,
    ErrErroneousNickname = 432,
    ErrNicknameInUse = 433,
    ErrNotOnChannel = 442,
    ErrNeedMoreParams = 461,
    ErrAlreadyRegistered = 462,
    ErrPasswordMismatch = 464,
}
