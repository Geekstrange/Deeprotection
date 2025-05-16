#!/bin/bash

# 定义需要检查的命令数组
commands=("curl" "jq" "bc" "awk" "dpkg")

# 存储缺失的命令
missing=()

# 检查每个命令是否可用
for cmd in "${commands[@]}"; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        missing+=("$cmd")
    fi
done

# 如果有缺失的命令，提示用户并询问是否安装
if [ ${#missing[@]} -ne 0 ]; then
    printf "Preparing for installation: \033[32m${missing[*]}\033[0m\n\n"
    
    # 使用更清晰的颜色显示
    read -p "$(printf 'Are you ready? \033[32my\033[0m/\033[31mn\033[0m ') " answer
    
    if [[ "$answer" =~ ^[Yy]$ ]]; then  # 支持大小写
        # 尝试使用sudo安装，如果没有权限或sudo不存在则直接安装
        echo "Attempting installation..."
        if sudo apt install -y "${missing[@]}" 2>/dev/null || apt install -y "${missing[@]}"; then
            printf "\033[32m✓ Installation completed successfully.\033[0m\n"
        else
            echo "Installation failed. Please try to install manually."
            exit 1
        fi
    else
        echo "Installation cancelled."
        exit 1
    fi
fi

# 继续执行脚本的其他部分
printf "\033[33m:)\033[0m Great! All ready.\n"
