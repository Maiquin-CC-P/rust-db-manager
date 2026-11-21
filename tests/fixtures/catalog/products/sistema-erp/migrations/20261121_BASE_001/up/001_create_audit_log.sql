CREATE TABLE [dbo].[AuditLog] (
    [AuditId]      BIGINT        IDENTITY(1,1) NOT NULL CONSTRAINT [PK_AuditLog] PRIMARY KEY,
    [EventName]    NVARCHAR(100) NOT NULL,
    [PayloadJson]  NVARCHAR(MAX) NULL,
    [LoggedAtUtc]  DATETIME2     NOT NULL CONSTRAINT [DF_AuditLog_LoggedAtUtc] DEFAULT (SYSUTCDATETIME())
);
GO
