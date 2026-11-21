CREATE VIEW [dbo].[vw_ActiveCustomers]
AS
SELECT
    [CustomerId],
    [TaxId],
    [CompanyName],
    [CreatedAtUtc]
FROM [dbo].[Customers]
WHERE [IsActive] = 1;
GO

CREATE PROCEDURE [dbo].[usp_IssueInvoice]
    @CustomerId INT,
    @InvoiceNumber NVARCHAR(50),
    @TotalAmount DECIMAL(18, 2)
AS
BEGIN
    SET NOCOUNT ON;

    INSERT INTO [dbo].[Invoices] ([CustomerId], [InvoiceNumber], [TotalAmount])
    VALUES (@CustomerId, @InvoiceNumber, @TotalAmount);

    SELECT SCOPE_IDENTITY() AS [NewInvoiceId];
END;
GO
