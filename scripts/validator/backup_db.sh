#!/bin/bash

set -e

ACTION="backup"
SERVER=""
SOURCE_DIR=""
TARGET_DIR=""
DB_NAME="validator.db"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
DEFAULT_BACKUP_DIR="./validator_backup_${TIMESTAMP}"

while [[ $# -gt 0 ]]; do
    case $1 in
        -c|--connect)
            SERVER="$2"
            shift 2
            ;;
        -s|--source)
            SOURCE_DIR="$2"
            shift 2
            ;;
        -t|--target)
            TARGET_DIR="$2"
            shift 2
            ;;
        -n|--name)
            DB_NAME="$2"
            shift 2
            ;;
        backup|restore)
            ACTION="$1"
            shift
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

DB_FILES=("$DB_NAME" "$DB_NAME-wal" "$DB_NAME-shm")

case "$ACTION" in
    backup)
        if [ -z "$SOURCE_DIR" ]; then
            echo "Error: Source directory required"
            echo "Usage: $0 backup -s <source_dir> [-t <target_dir>] [-c <server>] [-n <db_name>]"
            exit 1
        fi

        TARGET_DIR="${TARGET_DIR:-$DEFAULT_BACKUP_DIR}"
        mkdir -p "$TARGET_DIR"

        if [ -n "$SERVER" ]; then
            for file in "${DB_FILES[@]}"; do
                if ssh "$SERVER" "[ -f $SOURCE_DIR/$file ]"; then
                    rsync -avz "$SERVER:$SOURCE_DIR/$file" "$TARGET_DIR/"
                fi
            done
        else
            for file in "${DB_FILES[@]}"; do
                if [ -f "$SOURCE_DIR/$file" ]; then
                    cp -p "$SOURCE_DIR/$file" "$TARGET_DIR/"
                fi
            done
        fi

        echo "Backup completed to $TARGET_DIR"
        ;;

    restore)
        if [ -z "$SOURCE_DIR" ] || [ -z "$TARGET_DIR" ]; then
            echo "Error: Source and target directories required"
            echo "Usage: $0 restore -s <source_dir> -t <target_dir> [-c <server>] [-n <db_name>]"
            exit 1
        fi

        if [ ! -d "$SOURCE_DIR" ]; then
            echo "Error: Source directory $SOURCE_DIR not found"
            exit 1
        fi

        if [ -n "$SERVER" ]; then
            ssh "$SERVER" "mkdir -p $TARGET_DIR"
            for file in "${DB_FILES[@]}"; do
                if [ -f "$SOURCE_DIR/$file" ]; then
                    rsync -avz "$SOURCE_DIR/$file" "$SERVER:$TARGET_DIR/"
                fi
            done
        else
            mkdir -p "$TARGET_DIR"
            for file in "${DB_FILES[@]}"; do
                if [ -f "$SOURCE_DIR/$file" ]; then
                    cp -p "$SOURCE_DIR/$file" "$TARGET_DIR/"
                fi
            done
        fi

        echo "Restore completed to $TARGET_DIR"
        ;;

    *)
        echo "Usage: $0 [backup|restore] -s <source_dir> [-t <target_dir>] [-c <server>] [-n <db_name>]"
        echo "  backup  - Copy database files from source to target"
        echo "  restore - Copy database files from source to target"
        echo ""
        echo "Options:"
        echo "  -s, --source   - Source directory path (required)"
        echo "  -t, --target   - Target directory path (optional for backup, required for restore)"
        echo "  -c, --connect  - SSH connection string (optional, for remote operations)"
        echo "  -n, --name     - Database filename (default: validator.db)"
        echo ""
        echo "Examples:"
        echo "  Local backup:   $0 backup -s /opt/basilica/data"
        echo "  Remote backup:  $0 backup -s /opt/basilica/data -c user@server"
        echo "  Local restore:  $0 restore -s ./validator_backup_20240101 -t /opt/basilica/data"
        echo "  Remote restore: $0 restore -s ./validator_backup_20240101 -t /opt/basilica/data -c user@server"
        exit 0
        ;;
esac
