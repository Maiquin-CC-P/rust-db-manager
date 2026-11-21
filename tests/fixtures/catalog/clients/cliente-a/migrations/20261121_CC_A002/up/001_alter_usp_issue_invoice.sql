CREATE OR ALTER PROCEDURE [dbo].[usp_IssueInvoice]
    @CustomerId INT,
    @InvoiceNumber NVARCHAR(50),
    @TotalAmount DECIMAL(18, 2)
AS
BEGIN
    SET NOCOUNT ON;

    INSERT INTO [dbo].[Invoices] ([CustomerId], [InvoiceNumber], [TotalAmount])
    VALUES (@CustomerId, @InvoiceNumber, @TotalAmount);

    DECLARE @NewId INT = SCOPE_IDENTITY();

    INSERT INTO [dbo].[AuditLog] ([EventName], [PayloadJson])
    VALUES (N'CLIENTE_A_INVOICE_ISSUED', CONCAT(N'{"invoiceId":', @NewId, N'}'));

    SELECT @NewId AS [NewInvoiceId];
END;
GO
