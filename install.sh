#!/bin/bash

REPO_OWNER="Geekstrange"
REPO_NAME="Deeprotection"
DEB_PACKAGE_NAME="deeprotection"
DOWNLOAD_DIR="./"
MAX_RETRY=3    # max retries 最大重试次数
RETRY_DELAY=5  # Retry download time 重试下载时间

# Get the latest release information on GitHub
# 在GitHub上获取最新发布信息
LATEST_RELEASE=$(curl -s "https://api.github.com/repos/$REPO_OWNER/$REPO_NAME/releases/latest")

# Find and download the. zip file
# 查找并下载.zip文件
DEB_ASSET=$(echo "$LATEST_RELEASE" | jq -r '.assets[] | select(.name | endswith(".zip")) | .browser_download_url')
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

get_release() {
	if [ -n "$DEB_ASSET" ]; then
		for ((i = 1; i <= $MAX_RETRY; i++)); do
			echo "Attempting download (Attempt $i)..."
			if curl -L -o "$DOWNLOAD_DIR/$(basename "$DEB_ASSET")" "$DEB_ASSET"; then
				echo "Download Successful"
				# Verify if the file exists
				# 验证文件是否存在
				if [ ! -f "$DOWNLOAD_DIR/$DEB_ASSET" ]; then
					echo "Error: Downloaded but file not found"
					exit 1
				else
					unzip -qo $DOWNLOAD_DIR/$DEB_ASSET -d $DOWNLOAD_DIR
					# Attempt to install using sudo. If you do not have permission or sudo does not exist, install directly
					# 尝试使用sudo安装,如果没有权限或sudo不存在则直接安装
					if sudo dpkg -i $DOWNLOAD_DIR/*.deb 2>/dev/null && rm -f $DOWNLOAD_DIR/$DEB_ASSET $DOWNLOAD_DIR/*.deb || dpkg -i $DOWNLOAD_DIR/*.deb && rm -f $DOWNL OAD_DIR/$DEB_ASSET $DOWNLOAD_DIR/*.deb; then
						printf "\033[32mInstallation completed successfully\033[0m\n"
					fi
				fi
			else
				echo "Download failed. Retry after waiting for ${RETRY_DELAY} seconds..."
				sleep $RETRY_DELAY
			fi
		done
	else
		printf 'Error: File not found \e]8;;https://github.com/Geekstrange/Deeprotection/issues\a提交Issues\e]8;;\a.\n'
		exit 1
	fi
}
get_dependencies() {
	# Attempt to install using sudo. If you do not have permission or sudo does not exist, install directly
	# 尝试使用sudo安装,如果没有权限或sudo不存在则直接安装
	if sudo apt install -y "${missing[@]}" 2>/dev/null || apt install -y "${missing[@]}"; then
		printf "\033[32m✓ Installation completed successfully.\033[0m\n"
	else
		echo "Installation failed. Please try to install manually."
		exit 1
	fi
}

printf "Preparing for installation"

read -p "$(printf 'Are you ready? (\033[32my\033[0m)es/(\033[31mn\033[0m)o:') " answer

if [[ "$answer" =~ ^[Yy]$ ]]; then
	if [[ ${#missing[@]} -eq 0 ]]; then
		printf "Install \033[32mdeeprotection\033[0m\n"
		get_release
		printf "\033[33m:)\033[0m All ready.\n"
		exit 1
	else
		printf "Install dependencies: \033[32m${missing[*]}\033[0m\n"
		get_dependencies
		printf "Install \033[32mdeeprotection\033[0m\n"
		get_release
		printf "\033[33m:)\033[0m All ready.\n"
		exit 1
	fi
else
	echo "Cancelled"
	exit 1
fi
