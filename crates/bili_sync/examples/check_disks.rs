use sysinfo::{DiskRefreshKind, Disks};

fn main() {
    let mut disks = Disks::new();
    disks.refresh_specifics(true, DiskRefreshKind::nothing().with_storage());

    println!("检测到的所有磁盘/分区：\n");
    println!("{:<15} {:<10} {:<15} {:<15} {:<15} {:<10} {:<40}",
             "名称", "挂载点", "类型", "总容量(GB)", "可用(GB)", "可移动", "文件系统");
    println!("{}", "=".repeat(125));

    let mut total = 0u64;
    let mut available = 0u64;

    for disk in disks.iter() {
        let total_gb = disk.total_space() as f64 / 1024.0 / 1024.0 / 1024.0;
        let available_gb = disk.available_space() as f64 / 1024.0 / 1024.0 / 1024.0;

        println!("{:<15} {:<10} {:<15} {:<15.2} {:<15.2} {:<10} {:<40}",
                 disk.name().to_string_lossy(),
                 disk.mount_point().to_string_lossy(),
                 format!("{:?}", disk.kind()),
                 total_gb,
                 available_gb,
                 disk.is_removable(),
                 disk.file_system().to_string_lossy());

        total += disk.total_space();
        available += disk.available_space();
    }

    println!("\n{}", "=".repeat(125));
    println!("累加总计：");
    println!("  总容量: {:.2} GB ({:.2} TB)",
             total as f64 / 1024.0 / 1024.0 / 1024.0,
             total as f64 / 1024.0 / 1024.0 / 1024.0 / 1024.0);
    println!("  可用空间: {:.2} GB ({:.2} TB)",
             available as f64 / 1024.0 / 1024.0 / 1024.0,
             available as f64 / 1024.0 / 1024.0 / 1024.0 / 1024.0);

    println!("\n{}", "=".repeat(125));
    println!("原始字节值检查：");
    for disk in disks.iter() {
        println!("{:<10} total_space={} bytes, available_space={} bytes",
                 disk.mount_point().to_string_lossy(),
                 disk.total_space(),
                 disk.available_space());
    }
}
