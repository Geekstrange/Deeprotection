#!/bin/bash

# Define the command array that needs to be checked
# 定义需要检查的命令数组
commands=("curl" "jq" "bc" "awk" "dpkg" "unzip")

# Store missing commands
# 存储缺失的命令
missing=()

# Check if each command is available
# 检查每个命令是否可用
for cmd in "${commands[@]}"; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        missing+=("$cmd")
    fi
done

# If there are missing commands, prompt the user and ask if to install them
# 如果有缺失的命令,提示用户并询问是否安装
if [ ${#missing[@]} -ne 0 ]; then
    printf "Preparing for installation: \033[32m${missing[*]}\033[0m\n\n"
    
	read -p "$(printf 'Are you ready? (\033[32my\033[0m)es/(\033[31mn\033[0m)o:') " answer
    
    if [[ "$answer" =~ ^[Yy]$ ]]; then
		# Attempt to install using sudo. If you do not have permission or sudo does not exist, install directly
        # 尝试使用sudo安装,如果没有权限或sudo不存在则直接安装
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

printf "\033[33m:)\033[0m Great! All ready.\n"
