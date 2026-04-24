# SQL复杂度评估器 — 计算要素与公式规格说明


## 2. 评估维度总览

复杂度评估分为 **两大层级**：

### 2.1 SQL 语句级要素

| 要素 | 识别方式 | 适用方言 |
|------|----------|----------|
| 表引用数量 (tableCount) | 正则匹配 FROM/JOIN/INTO/UPDATE/DELETE 子句 | 全部 |
| JOIN 数量 (joinCount) | 正则匹配 `JOIN` 关键字 | 全部 |
| WHERE 条件数 (whereConditionCount) | 正则匹配 `AND`\|`OR` + 1 或仅检测 `WHERE` 存在性 | 全部 |
| 子查询数量 (subqueryCount) | 正则匹配 `(SELECT` 模式 | 全部 |
| 聚合函数数量 (aggregateFunctionCount) | 正则匹配 `SUM\|AVG\|COUNT\|MAX\|MIN (` 等 | 全部 |
| CASE 表达式数量 (caseExpressionCount) | 正则匹配 `CASE` 关键字 | 全部 |
| 集合操作数量 (setOperationCount) | 正则匹配 `UNION\|UNION ALL\|INTERSECT\|MINUS\|EXCEPT` | 全部 |
| GROUP BY 数量 (groupByCount) | 正则匹配 `GROUP BY` | 全部 |
| ORDER BY 数量 (orderByCount) | 正则匹配 `ORDER BY` | 全部 |
| 查询深度 (queryDepth) | 基于子查询数量计算 | 全部 |
| SQL Hint 数量 (hintCount) | 正则匹配 `/*+ ... */` | GaussDB |
| LATERAL VIEW 数量 | 正则匹配 `LATERAL VIEW` | Hive |
| DISTRIBUTE BY 数量 | 正则匹配 `DISTRIBUTE BY` | Hive |
| CLUSTER BY 数量 | 正则匹配 `CLUSTER BY` | Hive |
| SORT BY 数量 | 正则匹配 `SORT BY` | Hive |
| PARTITION BY 数量 | 正则匹配 `PARTITION BY` | Hive |
| 窗口函数数量 | 正则匹配 `OVER (` | Hive |
| UNION / UNION ALL 数量 | 分别匹配 | Hive |
| WITH 子句 (CTE) 数量 | 匹配 `WITH name AS (` | Hive |
| 嵌套 WITH 数量 | 匹配嵌套 WITH 模式 | Hive |

### 2.2 存储过程级要素

| 要素 | 识别方式 | 适用方言 |
|------|----------|----------|
| 循环数量 (loopCount) | 正则匹配 FOR/WHILE/LOOP 语句 | 全部 |
| 最大循环嵌套级别 (maxLoopNestingLevel) | 逐行跟踪 LOOP/END LOOP 深度 | 全部 |
| 自定义函数调用 (customFunctionCount) | 用户提供的函数名列表 + 正则匹配 | 全部 |
| 高权重表引用 (highWeightTableCount) | 用户提供的表名列表 + SQL 上下文匹配 | 全部 |
| 嵌套存储过程调用 (nestedProcedureCount) | 正则匹配过程调用模式，排除内置函数 | 全部 |
| 高权重过程调用 (highWeightProcedureCount) | 嵌套过程列表 ∩ 用户高权重列表 | 全部 |
| 游标数量 (cursorCount) | 正则匹配 CURSOR 声明/SYS_REFCURSOR/FOR...IN | 全部 |
| 游标操作数量 (cursorOperationCount) | 正则匹配 OPEN/FETCH/CLOSE 操作 | 全部 |
| 最大游标嵌套级别 (maxCursorNestingLevel) | 基于 DECLARE 块嵌套估算 | 全部 |
| 动态 SQL 数量 (dynamicSqlCount) | 匹配 EXECUTE IMMEDIATE / OPEN...FOR | GaussDB |
| 参数绑定数量 (paramBindingCount) | 匹配 `:param` 和 USING 子句 | GaussDB |
| 嵌套动态 SQL 深度 (nestedDynamicSqlCount) | EXECUTE IMMEDIATE 嵌套分析 | GaussDB |
| 事务控制数量 (transactionControlCount) | 匹配 COMMIT/ROLLBACK/SAVEPOINT | GaussDB |
| 事务嵌套级别 (transactionNestingLevel) | SAVEPOINT/ROLLBACK TO 嵌套分析 | GaussDB |
| 自治事务标志 (usesAutonomousTransactions) | 匹配 `PRAGMA AUTONOMOUS_TRANSACTION` | GaussDB |
| 子事务数量 (subtransactionCount) | 显式(SAVEPOINT+ROLLBACK) + 隐式(异常块+DML) | GaussDB |
| 最大子事务嵌套级别 | SAVEPOINT 栈深度跟踪 | GaussDB |
| Java 存储过程标志 | 匹配 `LANGUAGE JAVA` | GaussDB |
| Java 类型转换数量 | 匹配 `oracle.sql.*` / `java.lang.*` | GaussDB |
| 内置函数过滤结果 | gaussdb_functions.json（1316函数，44类别） | GaussDB |
| 无效 Hint 数量 | Hint 名称 vs 参考数据校验 | GaussDB |
| 源代码行数 (lineCount) | 按换行符分割计数 | 全部 |
| 包级别指标 | 过程数、变量数、Java过程检测 | GaussDB |

---


## 4. GaussDB 方言计算公式

### 4.1 SELECT 语句评分

```
overallScore = (tableCount × 10)
             + (joinCount × 15)
             + (whereConditionCount × 5)
             + (subqueryCount × 20)
             + (aggregateFunctionCount × 10)
             + (caseExpressionCount × 5)
             + (setOperationCount × 15)
             + (groupByCount × 5)
             + (orderByCount × 5)
             + (hintCount × 3)
```

**WHERE 条件计数规则**：`whereConditionCount = sql.toUpperCase().contains("WHERE") ? 1 : 0`（仅检测存在性，不计算条件数）。

**查询深度**：`queryDepth = 1 + subqueryCount`

### 4.2 非 SELECT 语句评分（INSERT/UPDATE/DELETE/MERGE）

```
overallScore = (tableCount × 10) + (hintCount × 3)
whereConditionCount = sql.toUpperCase().contains("WHERE") ? 1 : 0
```

注意：GaussDB 的非 SELECT 语句不使用 `log₁₀` 估算，而是直接基于表数量计算。

### 4.3 动态 SQL 评分

```
baseScore = log₁₀(sqlLength) × 5
overallScore = baseScore × (1 + 0.1 × tableCount) + (hintCount × 3)
```

### 4.4 CREATE TABLE 语句评分（GaussDB 专有）

```
overallScore = TABLE_WEIGHT(10)
             + (columnCount × 2)
             + (computedColumnCount × 15)
             + (checkConstraintCount × 10)
             + (filterResult.retainedCount × 5)
```

### 4.5 存储过程总评分

GaussDB 存储过程评分采用 **先累加后增强** 的策略。

#### 步骤一：所有 SQL 语句评分之和

```
overallScore = Σ(each_statement.overallScore)
```

#### 步骤二：循环复杂度

```
overallScore += (loopCount × 15) + (maxLoopNestingLevel × 20)
```

#### 步骤三：自定义函数复杂度

```
overallScore += customFunctionCount × 10
```

#### 步骤四：高权重表复杂度

```
overallScore += highWeightTableCount × 20
```

#### 步骤五：嵌套存储过程调用

```
overallScore += nestedProcedureCount × 15
```

#### 步骤六：高权重过程调用

```
若 highWeightProcedureCount > 0:
    overallScore += highWeightProcedureCount × 20
```

#### 步骤七：游标复杂度

```
cursorComplexity = cursorCount × 10
                 + cursorOperationCount × 5

若 maxCursorNestingLevel > 1:
    cursorComplexity = cursorComplexity × (1 + (maxCursorNestingLevel - 1) × 15)

overallScore += cursorComplexity
```

#### 步骤八：增强复杂度重新计算

GaussDB 在上述步骤之后，还会重新计算一个增强分数并 **取整覆盖** 原分数：

```
overallComplexity = (tableCount × 10)
                  + (joinCount × 15)
                  + (whereConditionCount × 5)
                  + (subqueryCount × 20)
                  + (setOperationCount × 15)
                  + (loopCount × 15)

baseScore = round(overallComplexity)
```

#### 步骤九：额外增强项

```
baseScore += dynamicSqlCount × 15
baseScore += paramBindingCount × 5
baseScore += nestedDynamicSqlCount × 25
baseScore += transactionControlCount × 10
baseScore += transactionNestingLevel × 20

若 usesAutonomousTransactions:
    baseScore += 15

baseScore += javaStoredProcedureCount × 25
baseScore += javaTypeConversionCount × 5
baseScore += hintCount × 3

若 packageMetrics != null:
    baseScore += packageMetrics.totalProcedures × 5
    baseScore += packageMetrics.packageLevelVariables × 2
    若 packageMetrics.containsJavaProcedures:
        baseScore += 25
```

#### 步骤十：最终分数

```
overallScore = baseScore   (int 类型，取整)
```

#### 步骤十一：Java 存储过程最低分保障

```
若 javaStoredProcedureCount > 0:
    overallScore = max(overallScore, 50)
```

### 4.6 GaussDB 权重常量表

| 常量 | 值 | 说明 |
|------|-----|------|
| TABLE_WEIGHT | 10 | 每个表引用 |
| JOIN_WEIGHT | 15 | 每个 JOIN |
| WHERE_CONDITION_WEIGHT | 5 | 每个 WHERE 条件 |
| SUBQUERY_WEIGHT | 20 | 每个子查询 |
| AGGREGATE_FUNCTION_WEIGHT | 10 | 每个聚合函数 |
| CASE_EXPRESSION_WEIGHT | 5 | 每个 CASE 表达式 |
| SET_OPERATION_WEIGHT | 15 | 每个集合操作 |
| GROUP_BY_WEIGHT | 5 | 每个 GROUP BY |
| ORDER_BY_WEIGHT | 5 | 每个 ORDER BY |
| LOOP_WEIGHT | 15 | 每个循环 |
| NESTED_LOOP_WEIGHT | 20 | 每层嵌套循环 |
| CUSTOM_FUNCTION_WEIGHT | 10 | 每个自定义函数调用 |
| HIGH_WEIGHT_TABLE_WEIGHT | 20 | 每个高权重表引用 |
| HIGH_WEIGHT_PROCEDURE_WEIGHT | 20 | 每个高权重过程调用 |
| NESTED_PROCEDURE_WEIGHT | 15 | 嵌套过程调用权重 |
| HINT_WEIGHT | 3 | 每个 SQL Hint |
| CURSOR_DECLARATION_WEIGHT | 10 | 游标声明 |
| CURSOR_OPERATION_WEIGHT | 5 | 游标操作 |
| NESTED_CURSOR_WEIGHT | 15 | 嵌套游标乘数 |
| DYNAMIC_SQL_WEIGHT | 15 | 动态 SQL |
| PARAMETER_BINDING_WEIGHT | 5 | 参数绑定 |
| EXECUTE_IMMEDIATE_WEIGHT | 20 | EXECUTE IMMEDIATE |
| NESTED_DYNAMIC_SQL_WEIGHT | 25 | 嵌套动态 SQL |
| TRANSACTION_CONTROL_WEIGHT | 10 | 事务控制语句 |
| AUTONOMOUS_TRANSACTION_WEIGHT | 15 | 自治事务 |
| NESTED_TRANSACTION_WEIGHT | 20 | 嵌套事务 |
| JAVA_PROCEDURE_WEIGHT | 25 | Java 存储过程 |
| TYPE_CONVERSION_WEIGHT | 5 | Java 类型转换 |
| COMPLEX_COLUMN_WEIGHT | 15 | 计算列 |
| CHECK_CONSTRAINT_WEIGHT | 10 | 检查约束 |

---

---

## 6. 三大方言权重对比表(供参考，请仅关注GaussDB)

### 6.1 SQL 语句级权重对比

| 要素 | Oracle | GaussDB | Hive |
|------|--------|---------|------|
| 表引用 | 1.0 | 10 | 1.0 |
| JOIN | 2.0 | 15 | 2.0 |
| WHERE 条件 | 1.5 | 5 | 1.5 |
| 子查询 | 3.0 | 20 | 3.0 |
| 聚合函数 | 1.0 | 10 | 1.0 |
| CASE 表达式 | 1.0 | 5 | 1.5 |
| 集合操作 | 2.0 | 15 | 2.0 |
| GROUP BY | 1.5 | 5 | 1.5 |
| ORDER BY | 1.0 | 5 | 1.0 |
| SQL Hint | — | 3 | — |
| LATERAL VIEW | — | — | 2.0 |
| DISTRIBUTE BY | — | — | 1.5 |
| CLUSTER BY | — | — | 1.5 |
| SORT BY | — | — | 1.0 |
| PARTITION BY | — | — | 1.5 |
| 窗口函数 | — | — | 2.5 |
| UNION | — | — | 2.5 |
| UNION ALL | — | — | 2.0 |
| WITH 子句(CTE) | — | — | 2.0 |
| WHERE 条件计数方式 | AND/OR数+1 | 仅检测有无 | 仅检测有无 |
| 查询深度计算 | subquery>0?2:1 | 1+subquery | max(1+subquery, unionDepth) |

### 6.2 存储过程级权重对比

| 要素 | Oracle | GaussDB | Hive |
|------|--------|---------|------|
| 循环 (LOOP_WEIGHT) | 2.5 | 15 | 仅统计不加分 |
| 嵌套循环 (NESTED_LOOP_WEIGHT) | 1.5(乘数) | 20(每层) | 仅统计不加分 |
| 自定义函数 (CUSTOM_FUNCTION_WEIGHT) | 2.0 | 10 | 仅统计不加分 |
| 高权重表 | 2.0(乘数) | 20(每引用) | 仅统计不加分 |
| 嵌套过程 (NESTED_PROCEDURE_WEIGHT) | 3.0 | 15 | 仅统计不加分 |
| 高权重过程 | 2.5 | 20 | 仅统计不加分 |
| 游标声明 (CURSOR_DECLARATION_WEIGHT) | 2.0 | 10 | 仅统计不加分 |
| 游标操作 (CURSOR_OPERATION_WEIGHT) | 1.5 | 5 | 仅统计不加分 |
| 嵌套游标 | 1.5(乘数) | 15(乘数) | 仅统计不加分 |
| 语句数量修正 | ×(1+0.1×n) | ✗ | ✗ |
| 动态 SQL | — | 15/次 | — |
| 参数绑定 | — | 5/次 | — |
| 嵌套动态 SQL | — | 25/次 | — |
| 事务控制 | — | 10/次 | — |
| 自治事务 | — | +15 | — |
| 嵌套事务 | — | 20/层 | — |
| Java 存储过程 | — | 25 + 最低50分 | — |
| Java 类型转换 | — | 5/次 | — |
| 包过程数 | — | 5/个 | — |
| 包变量数 | — | 2/个 | — |

### 6.3 非 SELECT 语句评分对比

| 方言 | 计算方式 |
|------|----------|
| GaussDB | `tableCount × 10 + hintCount × 3`（仅基于表数量） |


---

## 7. 通用检测机制

### 7.1 表名提取

所有方言均通过以下正则模式提取表名（去重后统计）：

| 模式 | 匹配目标 |
|------|----------|
| `FROM\s+([\w.]+)` | FROM 子句中的表 |
| `JOIN\s+([\w.]+)` | JOIN 子句中的表 |
| `INSERT\s+INTO\s+([\w.]+)` | INSERT 目标表 |
| `UPDATE\s+([\w.]+)` | UPDATE 目标表 |
| `DELETE\s+FROM\s+([\w.]+)` | DELETE 目标表（含 FROM） |
| `DELETE\s+([\w.]+)` | DELETE 目标表（不含 FROM） |

GaussDB 额外处理：
- 从整个存储过程源码中增强检测表名
- 排除 `%TYPE` 类型声明中的表引用（如 `v_table.column%TYPE`）
- 仅保留 DML 语句中实际使用的表



### 7.2 过程调用识别与过滤

存储过程调用的识别采用以下策略：

1. **正则匹配**：`(\w+)\s*\(` 模式匹配所有可能的调用
2. **排除过滤**：
   - 内置函数列表（，GaussDB ?00个）
   - SQL 关键字（SELECT/INSERT/UPDATE/DELETE 等）
   - PL/SQL 关键字（DECLARE/BEGIN/EXCEPTION 等）
   - 数据类型关键字（VARCHAR2/NUMBER/DATE 等）
   - 用户提供的自定义函数列表
   - 自身过程名（避免自引用）
   - 同包过程名（GaussDB）
3. **上下文过滤**（GaussDB）：
   - SQL 关键字后的名称视为表名（FROM/INTO/JOIN 等）
   - PROCEDURE/FUNCTION 定义中的名称视为声明
   - 字符串字面量中的匹配忽略
   - SQL Hint (`/*+ ... */`) 内的匹配忽略
4. **内置函数过滤**（GaussDB 专有）：
   - 加载 `gaussdb_functions.json`（1316 个函数，44 个类别）
   - 从过程调用列表中过滤已知内置函数
   - 返回过滤结果 `FunctionFilterResult`（含过滤数、保留数、类别分布）

### 7.3 注释移除（GaussDB）

GaussDB 在循环检测前会移除源代码中的注释：

- 单行注释：`--` 开头的行
- 多行注释：`/* ... */` 包围的内容

### 7.4 循环嵌套级别计算

GaussDB 使用逐行分析方法：

**GaussDB**：逐行扫描，检测 `FOR ... LOOP`、`WHILE ... LOOP`、独立 `LOOP` 开始，以及 `END LOOP` 结束，跟踪当前深度取最大值。

### 7.5 游标嵌套级别计算

所有方言使用相同的简化估算方法：
- 跟踪 `DECLARE` 块的开始（嵌套级别 +1）和 `END;` 块的结束（嵌套级别 -1）
- 若无检测到 DECLARE 嵌套但源码包含 `CURSOR`，则返回级别 1

---

## 8. 自定义因素说明

用户可通过 API 请求传入三类自定义因素：

### 8.1 自定义函数列表（customFunctions）

- **格式**：字符串列表
- **作用**：系统在源码中搜索这些函数名的调用（`\b函数名\s*\(` 模式）
- **影响**：

  - GaussDB: 每次 `customFunctionCount × 10` 加分

- **副作用**：自定义函数名会从嵌套过程调用检测结果中排除

### 8.2 高权重表列表（highWeightTables）

- **格式**：字符串列表（表名）
- **作用**：系统检测 SQL 语句中对这些表的引用
- **影响**：

  - GaussDB: `highWeightTableCount × 20` 加法加成


### 8.3 高权重过程列表（highWeightProcedures）

- **格式**：字符串列表（过程名）
- **作用**：系统检测嵌套过程调用中是否有这些高权重过程
- **影响**：

  - GaussDB: `highWeightProcedureCount × 20` 加法加成


---

---

## 附录 A：公式汇总伪代码

### GaussDB 存储过程完整公式

```
function gauss_sp_score(procedure):
    stmts = procedure.sqlStatements
    source = procedure.sourceCode
    
    // 1. SQL 语句评分之和
    stmtScore = sum(evaluate(stmt) for stmt in stmts)
    
    // 2. 循环复杂度（加法）
    stmtScore += loopCount * 15 + maxLoopNestingLevel * 20
    
    // 3. 自定义函数
    stmtScore += customFunctionCount * 10
    
    // 4. 高权重表
    stmtScore += highWeightTableCount * 20
    
    // 5. 嵌套过程
    stmtScore += nestedProcedureCount * 15
    
    // 6. 高权重过程
    if highWeightProcedureCount > 0:
        stmtScore += highWeightProcedureCount * 20
    
    // 7. 游标复杂度
    cursorCplx = cursorCount * 10 + cursorOpCount * 5
    if maxCursorNestingLevel > 1:
        cursorCplx *= (1 + (maxCursorNestingLevel - 1) * 15)
    stmtScore += cursorCplx
    
    // 8. 增强复杂度（覆盖分数）
    enhanced = tableCount*10 + joinCount*15 + whereCond*5
             + subqueryCount*20 + setOpCount*15 + loopCount*15
    baseScore = round(enhanced)
    
    // 9. GaussDB 特有增强项
    baseScore += dynamicSqlCount * 15
    baseScore += paramBindingCount * 5
    baseScore += nestedDynamicSqlCount * 25
    baseScore += transactionControlCount * 10
    baseScore += transactionNestingLevel * 20
    if usesAutonomousTransactions: baseScore += 15
    baseScore += javaSpCount * 25
    baseScore += javaTypeConvCount * 5
    baseScore += hintCount * 3
    if packageMetrics:
        baseScore += packageMetrics.procedures * 5
        baseScore += packageMetrics.variables * 2
        if hasJavaProcs: baseScore += 25
    
    // 10. Java 最低分保障
    if javaSpCount > 0:
        baseScore = max(baseScore, 50)
    
    return baseScore
```
