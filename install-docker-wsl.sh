#!/bin/bash

# 在WSL中安装Docker Engine的脚本

echo "🐳 开始在WSL中安装Docker Engine..."

# 更新包索引
sudo apt-get update

# 安装必要的包
sudo apt-get install -y \
    ca-certificates \
    curl \
    gnupg \
    lsb-release

# 添加Docker的官方GPG密钥
sudo mkdir -p /etc/apt/keyrings
curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg

# 设置仓库
echo \
  "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu \
  $(lsb_release -cs) stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null

# 更新包索引
sudo apt-get update

# 安装Docker Engine
sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin

# 启动Docker服务
sudo service docker start

# 将当前用户添加到docker组
sudo usermod -aG docker $USER

echo "✅ Docker安装完成！"
echo "📝 请运行以下命令重新登录以应用组权限："
echo "   exit"
echo "   wsl"
echo ""
echo "🚀 然后可以使用以下命令构建镜像："
echo "   docker build -t bili-sync ." 