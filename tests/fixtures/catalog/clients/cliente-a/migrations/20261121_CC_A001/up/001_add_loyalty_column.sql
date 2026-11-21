ALTER TABLE [dbo].[Customers]
ADD [LoyaltyTier] NVARCHAR(30) NOT NULL CONSTRAINT [DF_Customers_LoyaltyTier] DEFAULT (N'STANDARD');
GO
