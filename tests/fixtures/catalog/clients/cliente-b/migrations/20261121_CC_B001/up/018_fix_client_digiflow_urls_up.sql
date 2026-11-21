CREATE TABLE [dbo].[DigiflowEndpoints] (
    [EndpointId]  INT           IDENTITY(1,1) NOT NULL CONSTRAINT [PK_DigiflowEndpoints] PRIMARY KEY,
    [ServiceCode] NVARCHAR(50)  NOT NULL,
    [WebhookUrl]  NVARCHAR(500) NOT NULL,
    [IsEnabled]   BIT           NOT NULL CONSTRAINT [DF_DigiflowEndpoints_IsEnabled] DEFAULT (1)
);
GO

CREATE PROCEDURE [dbo].[usp_SyncDigiflowBilling]
    @InvoiceId INT
AS
BEGIN
    SET NOCOUNT ON;
    SELECT [EndpointId], [ServiceCode], [WebhookUrl]
    FROM [dbo].[DigiflowEndpoints]
    WHERE [IsEnabled] = 1;
END;
GO
