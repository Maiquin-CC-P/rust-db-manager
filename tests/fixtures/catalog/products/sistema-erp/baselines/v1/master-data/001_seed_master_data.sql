MERGE INTO [dbo].[SystemConfig] AS target
USING (
    VALUES
        (N'DEFAULT_CURRENCY', N'USD', 1),
        (N'TAX_RATE_PERCENT', N'18.00', 1),
        (N'ERP_BASELINE_VERSION', N'v1', 1)
) AS source ([ConfigKey], [ConfigValue], [IsSystem])
ON target.[ConfigKey] = source.[ConfigKey]
WHEN MATCHED AND target.[IsSystem] = 1 THEN
    UPDATE SET target.[ConfigValue] = source.[ConfigValue]
WHEN NOT MATCHED THEN
    INSERT ([ConfigKey], [ConfigValue], [IsSystem])
    VALUES (source.[ConfigKey], source.[ConfigValue], source.[IsSystem]);
GO
