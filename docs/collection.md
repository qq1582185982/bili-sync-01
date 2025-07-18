---
title: 合集与系列管理
editLink: false
---

# 合集与系列管理

`bili-sync` 能够同步B站的 "合集" 和 "系列"，这两种都是UP主创建的视频播放列表。

## 添加合集或系列

1.  在 Web UI 的侧边栏点击 "添加视频源"。
2.  在 "视频源类型" 下拉菜单中，选择 "合集"。
3.  **输入UP主ID (uid)**: 这是创建该合集或系列的UP主的ID。
    - 您可以手动输入ID，或通过下方的 **搜索功能** 按昵称查找UP主并自动填充。
4.  **选择合集/系列**:
    - 输入UP主ID后，系统会自动加载该UP主创建的所有合集和系列。
    - 从下拉列表中选择您想要同步的那一个。
5.  **选择合集类型**:
    - B站有两种形式的列表：**合集 (season)** 和 **系列 (series)**。
    - 请确保您在 "合集类型" 中选择了正确的类型，否则可能无法正确同步。通常，您可以根据播放页面的URL来判断。
6.  系统会自动填充名称，您也可以自定义。
7.  指定 **保存路径**。
8.  点击 "添加" 按钮。

## 管理合集/系列

与其它视频源一样，您可以在 "订阅管理" 页面对已添加的合集和系列进行管理，包括手动同步、编辑和删除。

# 获取视频合集/视频列表信息

视频合集和视频列表虽然在哔哩哔哩网站交互上行为类似，但在接口层级是两个不同的概念，程序配置中需要对两者做出区分。

## 配置形式与区分方法

在 bili-sync 的设计中，视频合集的 key 为 `season:{mid}:{season_id}`，而视频列表的 key 为 `series:{mid}:{series_id}`。

新版本 b 站网页端已经对两种类型做了初步整合，将需要的参数展示在了视频合集/视频列表的 URL 中，不再需要手动查看接口。URL 的路径格式为：


```
/{mid}/lists/{id}?type={season/series}
```

点开你想要订阅的视频合集/视频列表详情，查看 URL 即可拼接出对应的 key。

### 视频合集

![image](./assets/season.webp)

该视频合集的 key 为 `season:521722088:1987140`。

### 视频列表

![image](./assets/series.webp)

该视频列表的 key 为 `series:521722088:387214`。