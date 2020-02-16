pub enum NumericReply {
    RplWelcome = 1,
    RplYourHost = 2,
    RplCreated = 3,
    RplNoTopic = 331,
    RplTopic = 332,
    RplTopicSet = 333,
    RplVersion = 351,
    RplMotd = 372,
    RplMotdStart = 375,
    RplEndOfMotd = 376,
    RplYoureOper = 381,
    RplRehashing = 382,
    RplTime = 391,
    ErrNoSuchNick = 401,
    ErrNoSuchServer = 402,
    ErrNoSuchChannel = 403,
    ErrCannotSendToChan = 404,
    ErrNoRecipient = 411,
    ErrNoTextToSend = 412,
    ErrUnknownCommand = 421,
    ErrNoMotd = 422,
    ErrNoNicknameGiven = 431,
    ErrErroneousNickname = 432,
    ErrNicknameInUse = 433,
    ErrNotOnChannel = 442,
    ErrSummonDisabled = 445,
    ErrNeedMoreParams = 461,
    ErrAlreadyRegistered = 462,
    ErrPasswordMismatch = 464,
    ErrNoPrivileges = 481,
}
