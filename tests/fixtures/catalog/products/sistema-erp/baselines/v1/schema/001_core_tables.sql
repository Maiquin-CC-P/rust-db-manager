CREATE TABLE [dbo].[SystemConfig] (
    [ConfigKey]   NVARCHAR(100) NOT NULL CONSTRAINT [PK_SystemConfig] PRIMARY KEY,
    [ConfigValue] NVARCHAR(500) NOT NULL,
    [IsSystem]    BIT           NOT NULL CONSTRAINT [DF_SystemConfig_IsSystem] DEFAULT (1)
);
GO

CREATE TABLE [dbo].[Customers] (
    [CustomerId]   INT           IDENTITY(1,1) NOT NULL CONSTRAINT [PK_Customers] PRIMARY KEY,
    [TaxId]        NVARCHAR(20)  NOT NULL,
    [CompanyName]  NVARCHAR(200) NOT NULL,
    [IsActive]     BIT           NOT NULL CONSTRAINT [DF_Customers_IsActive] DEFAULT (1),
    [CreatedAtUtc] DATETIME2     NOT NULL CONSTRAINT [DF_Customers_CreatedAtUtc] DEFAULT (SYSUTCDATETIME())
);
GO

CREATE UNIQUE INDEX [UX_Customers_TaxId] ON [dbo].[Customers] ([TaxId]);
GO

CREATE TABLE [dbo].[Invoices] (
    [InvoiceId]    INT             IDENTITY(1,1) NOT NULL CONSTRAINT [PK_Invoices] PRIMARY KEY,
    [CustomerId]   INT             NOT NULL,
    [InvoiceNumber] NVARCHAR(50)   NOT NULL,
    [TotalAmount]  DECIMAL(18, 2)  NOT NULL,
    [IssuedAtUtc]  DATETIME2       NOT NULL CONSTRAINT [DF_Invoices_IssuedAtUtc] DEFAULT (SYSUTCDATETIME()),
    CONSTRAINT [FK_Invoices_Customers] FOREIGN KEY ([CustomerId]) REFERENCES [dbo].[Customers] ([CustomerId])
);
GO
