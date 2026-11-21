use rustdb_manager::db::batch_splitter::split_tsql_batches;

#[test]
fn splits_standard_go_batches() {
    let sql = r#"
CREATE TABLE [dbo].[T1] (Id INT NOT NULL PRIMARY KEY);
GO
CREATE VIEW [dbo].[V1] AS SELECT Id FROM [dbo].[T1];
go -- case-insensitive with trailing comment
INSERT INTO [dbo].[T1] (Id) VALUES (1);
GO
"#;

    let batches = split_tsql_batches(sql);
    assert_eq!(batches.len(), 3);
    assert!(batches[0].contains("CREATE TABLE [dbo].[T1]"));
    assert!(batches[1].contains("CREATE VIEW [dbo].[V1]"));
    assert!(batches[2].contains("INSERT INTO [dbo].[T1]"));
}

#[test]
fn ignores_go_inside_comments_strings_and_brackets() {
    let sql = r#"
CREATE PROCEDURE [dbo].[usp_TestGoParser]
AS
BEGIN
    /*
       Multi-line block comment containing a standalone GO line:
       GO
    */
    -- Single line comment with GO
    DECLARE @Msg NVARCHAR(100) = 'Line 1
GO
Line 3';
    SELECT @Msg AS [GO];
END;
GO
SELECT 1 AS [AfterProc];
"#;

    let batches = split_tsql_batches(sql);
    assert_eq!(
        batches.len(),
        2,
        "GO inside comments, string literals, or brackets must never split the batch"
    );
    assert!(batches[0].contains("CREATE PROCEDURE [dbo].[usp_TestGoParser]"));
    assert!(batches[0].contains("SELECT @Msg AS [GO];"));
    assert_eq!(batches[1], "SELECT 1 AS [AfterProc];");
}

#[test]
fn supports_go_repeat_count() {
    let sql = r#"
INSERT INTO [dbo].[Heartbeat] DEFAULT VALUES;
GO 3
"#;

    let batches = split_tsql_batches(sql);
    assert_eq!(batches.len(), 3);
}
