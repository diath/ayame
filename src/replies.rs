pub enum NumericReply {
    RplYoureOper = 381,
    ErrNoSuchNick = 401,
    ErrNoSuchChannel = 403,
    ErrNoRecipient = 411,
    ErrNoTextToSend = 412,
    ErrNoNicknameGiven = 431,
    ErrErroneousNickname = 432,
    ErrNicknameInUse = 433,
    ErrNeedMoreParams = 461,
    ErrAlreadyRegistered = 462,
    ErrPasswordMismatch = 464,
}
