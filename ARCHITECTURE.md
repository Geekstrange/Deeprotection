[English](# dp Architecture Technical Document)

[简体中文](# dp 架构技术文档)

# dp Architecture Technical Document

## Overall Architecture Diagram
```mermaid
graph TD
    A[User Input] --> B[Mode Checking Module]
    B --> C{Mode Selection}
    C -->|Enhanced| D[Command Pipeline Module]
    C -->|Permissive| E[Command Interception Module]
    D --> F[Path Protection Module]
    D --> G[Command Interception Module]
    D --> H[Deletion Confirmation Module]
    F --> I[Configuration File Parsing]
    G --> J[Regular Expression Matching]
    H --> K[Interactive Deletion]
```

## Core Module Explanation

### 1. Mode Checking Module (check_mode_module)
```mermaid
flowchart LR
    S[Entry Point] --> C1[Configuration File Parsing]
    C1 --> C2{mode Parameter}
    C2 -->|Enhanced| C3[Full Protection Chain]
    C2 -->|Permissive| C4[Basic Interception Mode]
```

- **Features**:
  - Supports two operation modes:
    - Enhanced: Full protection chain (default strict mode)
    - Permissive: Basic command interception only
  - Case-sensitive strict mode check
  - Automatic termination on configuration errors

### 2. Command Pipeline Module (command_pipeline_module)
```mermaid
sequenceDiagram
    User->>+Pipeline Module: Input Command
    Pipeline Module->>+Path Protection: Check Path
    Path Protection-->>-Pipeline Module: Return Status Code
    Pipeline Module->>+Command Interception: Pattern Matching
    Command Interception-->>-Pipeline Module: Replacement/Interception Result
    Pipeline Module->>+Deletion Confirmation: Interactive Processing
    Deletion Confirmation-->>-User: Final Execution Result
```

- **Plugin Extension Mechanism**:
  1. Add to configuration file `/etc/deeprotection/deeprotection.conf`:
   ```conf
   #command_intercept_rules
   original_command > replacement_command
   ```
  2. Supported regular expression matching rules:
   - `rm /` → `echo "protected"`
   - `chmod 777 *` → `""` (direct interception)

### 3. Path Protection Module (protected_paths_module)
```mermaid
classDiagram
    class PathValidator {
        +load_protected_paths()
        +realpath standardization()
        +prefix matching algorithm()
    }
```

- **Technical Implementation**:
  - Uses `realpath -m` for path standardization
  - Prefix matching algorithm time complexity: O(n)
  - Supports wildcard path configuration:
   ```conf
   #protected_paths_list
   /usr/lib/*
   /etc/passwd
   ```

### 4. Command Interception Module (command_intercept_module)
```mermaid
stateDiagram-v2
    [*] --> Command Parsing
    Command Parsing --> Regular Expression Matching: Full Command Matching
    Regular Expression Matching --> Interception Processing: On Match Success
    Interception Processing --> Replacement Execution: If Replacement Command Exists
    Interception Processing --> Full Interception: On Empty Replacement
    Regular Expression Matching --> Star Protection: Check for 'rm *' if No Match
```

- **Core Algorithm**:
  ```python
  def star_protection(files):
      current_files = os.listdir()
      if sorted(files) == sorted(current_files):
          return "Block 'rm *' operation"
  ```

### 5. Deletion Confirmation Module (rm_replace_module)
```mermaid
flowchart TB
    R[rm Command] --> R1[Argument Parsing]
    R1 --> R2{Contains -f parameter?}
    R2 -->|Yes| R3[Enforce Addition of -i parameter]
    R2 -->|No| R4[Retain Original Parameters]
    R3 --> R5[Interactive Deletion]
```

- **Forced Protection Mechanism**:
  - Always add `-i` for interactive confirmation regardless of parameters
  - Output format: `[!] About to execute: /bin/rm -i -v filename`
  - Visual warning: flashing red alert

## Logging System Design
```mermaid
gantt
    title Log Record Entries
    dateFormat  YYYY-MM-DD HH:mm:ss
    section Log Fields
    Timestamp           :a1, 2023-10-01 12:00:00, 1s
    User Identity       :after a1, 1s
    Full Command        :after a1, 2s
    Execution Path      :after a1, 3s
    PID Information     :after a1, 4s
    Exit Status Code    :after a1, 5s
```

- **Security Design**:
  - Automatic log directory creation (ACL: 700)
  - Log file permissions: `-rw-r-----`
  - Log injection protection: escape special characters

## Configuration File Example
```conf
#deeprotection.conf

#protected_paths_list
/etc/
/root/.ssh
/boot/

#command_intercept_rules
rm -rf /* > 
chmod 777 * > chmod 755
```

## Extension Development Guide

### Plugin Development Interface
```bash
# Custom Plugin Template
_my_plugin() {
    local command="$@"
    # Detection Logic
    if [[ "$command" =~ dangerous_pattern ]]; then
        output_log "[PLUGIN] Blocked Dangerous Command"
        return 1
    fi
    return 0
}

# Insert into Pipeline Module
command_pipeline_module() {
    _my_plugin "$@" || return 1
    protected_paths_module "$@" || return 1
    ...
}
```
---
# dp 架构技术文档

## 整体架构图
```mermaid
graph TD
    A[用户输入] --> B[模式检查模块]
    B --> C{模式选择}
    C -->|Enhanced| D[命令管道模块]
    C -->|Permissive| E[命令拦截模块]
    D --> F[路径保护模块]
    D --> G[命令拦截模块]
    D --> H[删除确认模块]
    F --> I[配置文件解析]
    G --> J[正则表达式匹配]
    H --> K[交互式删除]
```

## 核心模块说明

### 1. 模式检查模块 (check_mode_module)
```mermaid
flowchart LR
    S[启动入口] --> C1[配置文件解析]
    C1 --> C2{mode参数}
    C2 -->|Enhanced| C3[完整防护链]
    C2 -->|Permissive| C4[基础拦截模式]
```

- **功能特性**：
  - 支持两种运行模式：
    - Enhanced：完整防护链（默认严格模式）
    - Permissive：仅基础命令拦截
  - 严格模式检查（区分大小写）
  - 配置错误自动终止

### 2. 命令管道模块 (command_pipeline_module)
```mermaid
sequenceDiagram
    用户->>+管道模块: 输入命令
    管道模块->>+路径保护: 检查路径
    路径保护-->>-管道模块: 返回状态码
    管道模块->>+命令拦截: 模式匹配
    命令拦截-->>-管道模块: 替换/拦截结果
    管道模块->>+删除确认: 交互式处理
    删除确认-->>-用户: 最终执行结果
```

- **插件扩展机制**：
  1. 通过修改配置文件 `/etc/deeprotection/deeprotection.conf` 添加：
   ```conf
   #command_intercept_rules
   original_command > replacement_command
   ```
  2. 支持的正则表达式匹配规则：
   - `rm /` → `echo "protected"`
   - `chmod 777 *` → `""` (直接拦截)

### 3. 路径保护模块 (protected_paths_module)
```mermaid
classDiagram
    class PathValidator {
        +load_protected_paths()
        +realpath标准化()
        +前缀匹配算法()
    }
```

- **技术实现**：
  - 使用 `realpath -m` 进行路径标准化
  - 前缀匹配算法时间复杂度：O(n)
  - 支持通配符路径配置：
   ```conf
   #protected_paths_list
   /usr/lib/*
   /etc/passwd
   ```

### 4. 命令拦截模块 (command_intercept_module)
```mermaid
stateDiagram-v2
    [*] --> 命令解析
    命令解析 --> 正则匹配: 完整命令匹配
    正则匹配 --> 拦截处理: 匹配成功
    拦截处理 --> 替换执行: 存在替换命令
    拦截处理 --> 完全拦截: 空替换
    正则匹配 --> 星号保护: 未匹配时检查rm *
```

- **核心算法**：
  ```python
  def 星号保护(files):
      current_files = os.listdir()
      if sorted(files) == sorted(current_files):
          return "拦截rm *操作"
  ```

### 5. 删除确认模块 (rm_replace_module)
```mermaid
flowchart TB
    R[rm命令] --> R1[参数解析]
    R1 --> R2{包含-f参数?}
    R2 -->|是| R3[强制添加-i参数]
    R2 -->|否| R4[保留原始参数]
    R3 --> R5[交互式删除]
```

- **强制保护机制**：
  - 无论参数如何都添加 `-i` 交互确认
  - 输出格式：`[!] 即将执行: /bin/rm -i -v filename`
  - 视觉警告：闪烁红色提示

## 日志系统设计
```mermaid
gantt
    title 日志记录条目
    dateFormat  YYYY-MM-DD HH:mm:ss
    section 日志字段
    时间戳           :a1, 2023-10-01 12:00:00, 1s
    用户身份         :after a1, 1s
    完整命令         :after a1, 2s
    执行路径         :after a1, 3s
    PID信息         :after a1, 4s
    退出状态码       :after a1, 5s
```

- **安全设计**：
  - 日志目录自动创建 (ACL: 700)
  - 日志文件权限：`-rw-r-----`
  - 日志注入防护：特殊字符转义

## 配置文件示例
```conf
#deeprotection.conf

#protected_paths_list
/etc/
/root/.ssh
/boot/

#command_intercept_rules
rm -rf /* > 
chmod 777 * > chmod 755
```

## 扩展开发指南

### 插件开发接口
```bash
# 自定义插件模板
_my_plugin() {
    local command="$@"
    # 检测逻辑
    if [[ "$command" =~ dangerous_pattern ]]; then
        output_log "[PLUGIN] 拦截危险命令"
        return 1
    fi
    return 0
}

# 插入到管道模块
command_pipeline_module() {
    _my_plugin "$@" || return 1
    protected_paths_module "$@" || return 1
    ...
}
```
