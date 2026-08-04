# OMV 8 RPC 命令目录(omv-rpc 能力面 / AAOS 能力目录底座)

自动扫描自 openmediavault 源码:`engined/rpc/*.inc`(服务+方法) + `datamodels/rpc.*.json`(参数 schema)。
调用:`omv-rpc "<Service>" "<method>" '<json>'`(root 免登录;异步方法返回 /tmp/bgstatus* 句柄)。
新建 set 类方法 uuid 传哨兵 `fa4b1c66-ef79-11e5-87a0-0002b3a176b4`;写后 `omv-salt deploy run <service>` 才落系统。
**核心包:服务 31 | 方法 265**(插件 zfs/ftp 等另加)


## Apt (`apt`) - 11 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getSoftwareSettings` | read | _(无参/无 schema)_ |
| `setSoftwareSettings` | write | _(无参/无 schema)_ |
| `getUpdatesSettings` | read | _(无参/无 schema)_ |
| `setUpdatesSettings` | write | _(无参/无 schema)_ |
| `enumerateUpgraded` | read | _(无参/无 schema)_ |
| `getUpgradedList` | read | _(无参/无 schema)_ |
| `install` | write | `packages`:array* |
| `upgrade` | write | _(无参/无 schema)_ |
| `update` | write | _(无参/无 schema)_ |
| `upload` | write | `filename`:string*, `filepath`:string* |
| `getChangeLog` | read | `filename`:string* |

## Certificatemgmt (`certificatemgmt`) - 12 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getList` | read | _(无参/无 schema)_ |
| `get` | read | _(无参/无 schema)_ |
| `set` | write | `uuid`:string*, `certificate`:string*, `privatekey`:string, `comment`:string* |
| `delete` | destructive | _(无参/无 schema)_ |
| `getDetail` | read | _(无参/无 schema)_ |
| `create` | write | `size`:integer*512/1024/2048/4096, `days`:integer*, `c`:string*, `st`:string*, `l`:string*, `o`:string*, `ou`:string*, `cn`:string*, `email`:string* |
| `getSshList` | read | _(无参/无 schema)_ |
| `createSsh` | write | `type`:string*rsa/ed25519, `comment`:string |
| `getSsh` | read | _(无参/无 schema)_ |
| `setSsh` | write | `uuid`:string*, `publickey`:string*, `privatekey`:string, `comment`:string* |
| `deleteSsh` | destructive | _(无参/无 schema)_ |
| `copySshId` | write | `uuid`:string*, `hostname`:string*, `port`:integer*, `username`:string*, `password`:string* |

## Config (`config`) - 9 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `isDirty` | read | `modules`:array |
| `applyChanges` | write | `modules`:array*, `force`:boolean* |
| `applyChangesBg` | write | _(无参/无 schema)_ |
| `revertChanges` | write | `filename`:string |
| `revertChangesBg` | write | _(无参/无 schema)_ |
| `getlist` | read | `id`:string*, `start`:integer*, `limit`:['integer', 'null']*, `sortfield`:['string', 'null'], `sortdir`:['string', 'null']asc/ASC/desc/DESC, `search`:['string', 'integer', 'null'] |
| `get` | read | `id`:string*, `uuid`:string |
| `set` | write | `id`:string*, `data`:object* |
| `delete` | destructive | `id`:string*, `uuid`:string |

## Cron (`cron`) - 5 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getList` | read | `start`:integer*, `limit`:['integer', 'null']*, `sortfield`:['string', 'null'], `sortdir`:['string', 'null']asc/ASC/desc/DESC, `type`:array* |
| `get` | read | _(无参/无 schema)_ |
| `set` | write | `uuid`:string*, `enable`:boolean*, `execution`:string*exactly/hourly/daily/weekly/monthly/yearly/reboot, `sendemail`:boolean*, `comment`:string*, `type`:string*reboot/shutdown/standby/userdefined, `minute`:array*, `everynminute`:boolean*, `hour`:array*, `everynhour`:boolean*, `month`:array*, `dayofmonth`:array*, `everyndayofmonth`:boolean*, `dayofweek`:array*, `username`:string*, `command`:string* |
| `delete` | destructive | _(无参/无 schema)_ |
| `execute` | write | _(无参/无 schema)_ |

## Diskmgmt (`diskmgmt`) - 7 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `enumerateDevices` | read | _(无参/无 schema)_ |
| `getList` | read | _(无参/无 schema)_ |
| `getListBg` | read | _(无参/无 schema)_ |
| `getHdParm` | read | _(无参/无 schema)_ |
| `setHdParm` | write | `uuid`:string*, `devicefile`:string*, `apm`:integer*, `aam`:integer*0/128/254, `spindowntime`:integer*, `writecache`:boolean* |
| `wipe` | destructive | `devicefile`:string*, `secure`:boolean* |
| `rescan` | read | _(无参/无 schema)_ |

## Envvars (`envvars`) - 7 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `enumerate` | read | _(无参/无 schema)_ |
| `getList` | read | _(无参/无 schema)_ |
| `get` | read | `name`:string* |
| `set` | write | `name`:string*, `value`:string* |
| `delete` | destructive | `name`:string* |
| `apply` | write | _(无参/无 schema)_ |
| `applyBg` | write | _(无参/无 schema)_ |

## Exec (`exec`) - 5 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `stop` | write | `filename`:string* |
| `getOutput` | read | `filename`:string*, `pos`:integer*, `length`:integer |
| `isRunning` | read | `filename`:string* |
| `enumerate` | read | _(无参/无 schema)_ |
| `attach` | write | `filename`:string* |

## Filesystemmgmt (`filesystemmgmt`) - 15 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `enumerateFilesystems` | read | _(无参/无 schema)_ |
| `enumerateMountedFilesystems` | read | `type`:string, `includeroot`:boolean |
| `getList` | read | _(无参/无 schema)_ |
| `getListBg` | read | _(无参/无 schema)_ |
| `getCandidates` | read | _(无参/无 schema)_ |
| `getCandidatesBg` | read | _(无参/无 schema)_ |
| `getMountCandidates` | read | _(无参/无 schema)_ |
| `create` | write | `devicefile`:string*, `type`:string* |
| `createBtrfs` | write | `label`:string, `profile`:string*single/dup/raid0/raid1/raid10, `devicefiles`:string* |
| `grow` | write | `id`:string* |
| `setMountPoint` | write | `id`:string*, `usagewarnthreshold`:number, `comment`:string |
| `umountByFsName` | destructive | `fsname`:string* |
| `umountByDir` | destructive | `dir`:string* |
| `hasFilesystem` | read | `devicefile`:string* |
| `getDetails` | read | `devicefile`:string* |

## Folderbrowser (`folderbrowser`) - 1 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `get` | read | `uuid`:string*, `type`:string*mntent/sharedfolder, `path`:string* |

## Fstab (`fstab`) - 6 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `enumerateEntries` | read | _(无参/无 schema)_ |
| `get` | read | _(无参/无 schema)_ |
| `set` | write | `uuid`:string*, `fsname`:string*, `dir`:string*, `type`:string*, `opts`:string*, `freq`:integer*, `passno`:integer*0/1/2, `usagewarnthreshold`:integer, `comment`:string |
| `delete` | destructive | _(无参/无 schema)_ |
| `getByFsName` | read | `fsname`:string* |
| `getByDir` | read | `dir`:string* |

## Iptables (`iptables`) - 7 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getRules` | read | _(无参/无 schema)_ |
| `setRules` | write | _(无参/无 schema)_ |
| `getRules6` | read | _(无参/无 schema)_ |
| `setRules6` | write | _(无参/无 schema)_ |
| `getRule` | read | _(无参/无 schema)_ |
| `setRule` | write | `uuid`:string*, `rulenum`:integer*, `chain`:string*INPUT/OUTPUT, `action`:string*ACCEPT/REJECT/DROP/LOG/, `family`:string*inet/inet6, `source`:string*, `sport`:string*, `destination`:string*, `dport`:string*, `protocol`:string*, `extraoptions`:string*, `comment`:string* |
| `deleteRule` | destructive | _(无参/无 schema)_ |

## Logfile (`logfile`) - 3 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getList` | read | `start`:integer*, `limit`:['integer', 'null']*, `sortfield`:['string', 'null'], `sortdir`:['string', 'null']asc/ASC/desc/DESC, `id`:string* |
| `clear` | write | `id`:string* |
| `getContent` | read | `id`:string* |

## Network (`network`) - 27 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getGeneralSettings` | read | _(无参/无 schema)_ |
| `setGeneralSettings` | write | `hostname`:string*, `domainname`:string* |
| `enumerateDevices` | read | _(无参/无 schema)_ |
| `enumerateDevicesList` | read | _(无参/无 schema)_ |
| `enumerateConfiguredDevices` | read | _(无参/无 schema)_ |
| `identify` | write | `devicename`:string*, `seconds`:integer* |
| `getInformation` | read | `devicename`:string* |
| `getInterfaceList` | read | _(无参/无 schema)_ |
| `getInterface` | read | _(无参/无 schema)_ |
| `deleteInterface` | destructive | _(无参/无 schema)_ |
| `getEthernetCandidates` | read | _(无参/无 schema)_ |
| `getEthernetIface` | read | _(无参/无 schema)_ |
| `setEthernetIface` | write | `uuid`:string*, `devicename`:string*, `method`:string*dhcp/static/manual, `address`:string*, `netmask`:string*, `gateway`:string*, `routemetric`:integer*, `method6`:string*auto/static/manual/dhcp, `address6`:string*, `netmask6`:integer*, `gateway6`:string*, `routemetric6`:integer*, `dnsnameservers`:string*, `dnssearch`:string*, `mtu`:integer*, `wol`:boolean*, `comment`:string*, `altmacaddress`:string* |
| `enumerateBondSlaves` | read | `uuid`:string*, `unused`:boolean* |
| `getBondIface` | read | _(无参/无 schema)_ |
| `setBondIface` | write | `uuid`:string*, `devicename`:string*, `method`:string*dhcp/static/manual, `address`:string*, `netmask`:string*, `gateway`:string*, `routemetric`:integer*, `method6`:string*auto/static/manual/dhcp, `address6`:string*, `netmask6`:integer*, `gateway6`:string*, `routemetric6`:integer*, `dnsnameservers`:string*, `dnssearch`:string*, `mtu`:integer*, `wol`:boolean*, `comment`:string*, `slaves`:array*, `bondprimary`:string*, `bondmode`:integer*0/1/2/3/4/5/6, `bondmiimon`:integer*, `bonddowndelay`:integer*, `bondupdelay`:integer* |
| `getVlanCandidates` | read | _(无参/无 schema)_ |
| `getVlanIface` | read | _(无参/无 schema)_ |
| `setVlanIface` | write | `uuid`:string*, `devicename`:string*, `method`:string*dhcp/static/manual, `address`:string*, `netmask`:string*, `gateway`:string*, `routemetric`:integer*, `method6`:string*auto/static/manual/dhcp, `address6`:string*, `netmask6`:integer*, `gateway6`:string*, `routemetric6`:integer*, `dnsnameservers`:string*, `dnssearch`:string*, `mtu`:integer*, `wol`:boolean*, `comment`:string*, `altmacaddress`:string*, `vlanid`:integer*, `vlanrawdevice`:string* |
| `getWirelessCandidates` | read | _(无参/无 schema)_ |
| `getWirelessIface` | read | _(无参/无 schema)_ |
| `setWirelessIface` | write | `uuid`:string*, `devicename`:string*, `method`:string*dhcp/static/manual, `address`:string*, `netmask`:string*, `gateway`:string*, `routemetric`:integer*, `method6`:string*auto/static/manual/dhcp, `address6`:string*, `netmask6`:integer*, `gateway6`:string*, `routemetric6`:integer*, `dnsnameservers`:string*, `dnssearch`:string*, `mtu`:integer*, `wol`:boolean*, `comment`:string*, `altmacaddress`:string*, `band`:string*auto/2.4GHz/5GHz, `wpassid`:string*, `wpapsk`:string*, `keymanagement`:string*psk/sae, `hidden`:boolean* |
| `enumerateBridgeSlaves` | read | `uuid`:string*, `unused`:boolean* |
| `getBridgeIface` | read | _(无参/无 schema)_ |
| `setBridgeIface` | write | `uuid`:string*, `devicename`:string*, `method`:string*dhcp/static/manual, `address`:string*, `netmask`:string*, `gateway`:string*, `routemetric`:integer*, `method6`:string*auto/static/manual/dhcp, `address6`:string*, `netmask6`:integer*, `gateway6`:string*, `routemetric6`:integer*, `dnsnameservers`:string*, `dnssearch`:string*, `mtu`:integer*, `wol`:boolean*, `comment`:string*, `slaves`:array* |
| `getProxy` | read | _(无参/无 schema)_ |
| `setProxy` | write | `httpenable`:boolean*, `httphost`:string*, `httpport`:integer*, `httpusername`:string*, `httppassword`:string*, `httpsenable`:boolean*, `httpshost`:string*, `httpsport`:integer*, `httpsusername`:string*, `httpspassword`:string*, `ftpenable`:boolean*, `ftphost`:string*, `ftpport`:integer*, `ftpusername`:string*, `ftppassword`:string* |

## Nfs (`nfs`) - 7 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getSettings` | read | _(无参/无 schema)_ |
| `setSettings` | write | `enable`:boolean*, `versions`:array* |
| `getShareList` | read | _(无参/无 schema)_ |
| `getShare` | read | _(无参/无 schema)_ |
| `setShare` | write | `uuid`:string*, `sharedfolderref`:string*, `mntentref`:string*, `client`:string*, `options`:string*, `extraoptions`:string*, `comment`:string* |
| `deleteShare` | destructive | _(无参/无 schema)_ |
| `getStats` | read | _(无参/无 schema)_ |

## Notification (`notification`) - 8 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getList` | read | _(无参/无 schema)_ |
| `setList` | write | _(无参/无 schema)_ |
| `get` | read | _(无参/无 schema)_ |
| `set` | write | `uuid`:string*, `id`:string*, `enable`:boolean* |
| `isEnabled` | read | `id`:string* |
| `get` | read | _(无参/无 schema)_ |
| `set` | write | `uuid`:string*, `id`:string*, `enable`:boolean* |
| `sendTestEmail` | write | _(无参/无 schema)_ |

## Perfstats (`perfstats`) - 2 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `get` | read | _(无参/无 schema)_ |
| `set` | write | `enable`:boolean* |

## Pluginmgmt (`pluginmgmt`) - 5 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `enumeratePlugins` | read | _(无参/无 schema)_ |
| `getList` | read | _(无参/无 schema)_ |
| `remove` | destructive | _(无参/无 schema)_ |
| `upload` | write | _(无参/无 schema)_ |
| `install` | write | _(无参/无 schema)_ |

## Powermgmt (`powermgmt`) - 8 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `get` | read | _(无参/无 schema)_ |
| `set` | write | `cpufreqgovernor`:string*, `powerbtn`:string*nothing/shutdown/standby |
| `getScheduleList` | read | _(无参/无 schema)_ |
| `getScheduledJob` | read | _(无参/无 schema)_ |
| `setScheduledJob` | write | `uuid`:string*, `enable`:boolean*, `type`:['string', 'null']*reboot/shutdown/standby, `execution`:string*exactly/hourly/daily/weekly/monthly/yearly, `minute`:array*, `everynminute`:boolean*, `hour`:array*, `everynhour`:boolean*, `month`:array*, `dayofmonth`:array*, `everyndayofmonth`:boolean*, `dayofweek`:array*, `comment`:string* |
| `deleteScheduledJob` | destructive | _(无参/无 schema)_ |
| `executeScheduledJob` | write | _(无参/无 schema)_ |
| `enumerateStandbyModes` | read | _(无参/无 schema)_ |

## Quota (`quota`) - 5 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `get` | read | _(无参/无 schema)_ |
| `set` | write | _(无参/无 schema)_ |
| `getByTypeName` | read | `uuid`:string*, `name`:string*, `type`:string*user/group |
| `setByTypeName` | write | `uuid`:string*, `name`:string*, `type`:string*user/group, `bhardlimit`:number*, `bunit`:string*B/KiB/MiB/GiB/TiB/PiB/EiB |
| `delete` | destructive | _(无参/无 schema)_ |

## Rrd (`rrd`) - 2 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `generate` | write | _(无参/无 schema)_ |
| `getGraph` | read | `kind`:string*, `period`:string*hour/day/week/month/year |

## Rsync (`rsync`) - 5 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getList` | read | _(无参/无 schema)_ |
| `get` | read | _(无参/无 schema)_ |
| `set` | write | `uuid`:string*, `enable`:boolean*, `sendemail`:boolean*, `comment`:string*, `type`:string*local/remote, `srcsharedfolderref`:string*, `srcuri`:string*, `destsharedfolderref`:string*, `desturi`:string*, `minute`:array*, `everynminute`:boolean*, `hour`:array*, `everynhour`:boolean*, `month`:array*, `dayofmonth`:array*, `everyndayofmonth`:boolean*, `dayofweek`:array*, `optionrecursive`:boolean*, `optiontimes`:boolean*, `optiongroup`:boolean*, `optionowner`:boolean*, `optioncompress`:boolean*, `optionarchive`:boolean*, `optiondelete`:boolean*, `optionquiet`:boolean*, `optionperms`:boolean*, `optionacls`:boolean*, `optionxattrs`:boolean*, `optiondryrun`:boolean*, `optionpartial`:boolean*, `extraoptions`:string*, `mode`:string*push/pull, `authentication`:string*password/pubkey, `password`:string*, `sshcertificateref`:string*, `sshport`:integer* |
| `delete` | destructive | _(无参/无 schema)_ |
| `execute` | write | _(无参/无 schema)_ |

## Rsyncd (`rsyncd`) - 6 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getSettings` | read | _(无参/无 schema)_ |
| `setSettings` | write | `enable`:boolean*, `port`:integer*, `extraoptions`:string* |
| `getModuleList` | read | _(无参/无 schema)_ |
| `getModule` | read | _(无参/无 schema)_ |
| `setModule` | write | `uuid`:string*, `enable`:boolean*, `sharedfolderref`:string*, `name`:string*, `comment`:string*, `uid`:string*, `gid`:string*, `readonly`:boolean*, `writeonly`:boolean*, `maxconnections`:integer*, `list`:boolean*, `hostsallow`:string*, `hostsdeny`:string*, `authusers`:boolean*, `usechroot`:boolean*, `users`:array*, `extraoptions`:string* |
| `deleteModule` | destructive | _(无参/无 schema)_ |

## Services (`services`) - 1 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getStatus` | read | _(无参/无 schema)_ |

## Sharemgmt (`sharemgmt`) - 25 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getCandidates` | read | _(无参/无 schema)_ |
| `enumerateSharedFolders` | read | _(无参/无 schema)_ |
| `getList` | read | _(无参/无 schema)_ |
| `get` | read | _(无参/无 schema)_ |
| `set` | write | `uuid`:string*, `name`:string*, `reldirpath`:string*, `comment`:string*, `mntentref`:string*, `mode`:string700/750/755/770/775/777 |
| `delete` | destructive | `uuid`:string*, `recursive`:boolean* |
| `getPrivileges` | read | _(无参/无 schema)_ |
| `setPrivileges` | write | `uuid`:string*, `privileges`:array |
| `getPrivilegesByRole` | read | `role`:string*user/group, `name`:string* |
| `setPrivilegesByRole` | write | `role`:string*user/group, `name`:string*, `privileges`:array |
| `copyPrivileges` | write | `src`:string*, `dst`:string* |
| `getFileACL` | read | `uuid`:string*, `file`:string* |
| `setFileACL` | write | `uuid`:string*, `file`:string*, `recursive`:boolean*, `replace`:boolean*, `owner`:string, `group`:string, `userperms`:integer0/1/2/3/4/5/6/7, `groupperms`:integer0/1/2/3/4/5/6/7, `otherperms`:integer0/1/2/3/4/5/6/7, `users`:array, `groups`:array |
| `getPath` | read | _(无参/无 schema)_ |
| `enumerateSnapshots` | read | _(无参/无 schema)_ |
| `enumerateAllSnapshots` | read | _(无参/无 schema)_ |
| `createSnapshot` | write | _(无参/无 schema)_ |
| `createScheduledSnapshotTask` | write | _(无参/无 schema)_ |
| `enumerateScheduledSnapshotTasks` | read | _(无参/无 schema)_ |
| `enumerateAllScheduledSnapshotTasks` | read | _(无参/无 schema)_ |
| `deleteSnapshot` | destructive | `uuid`:string*, `id`:['string', 'integer']* |
| `restoreSnapshot` | write | `uuid`:string*, `id`:['string', 'integer']* |
| `fromSnapshot` | write | `uuid`:string*, `id`:['string', 'integer']* |
| `getSnapshotLifecycle` | read | _(无参/无 schema)_ |
| `setSnapshotLifecycle` | write | _(无参/无 schema)_ |

## Smart (`smart`) - 17 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `enumerateDevices` | read | _(无参/无 schema)_ |
| `enumerateMonitoredDevices` | read | _(无参/无 schema)_ |
| `getList` | read | _(无参/无 schema)_ |
| `getListBg` | read | _(无参/无 schema)_ |
| `getSettings` | read | _(无参/无 schema)_ |
| `setSettings` | write | `enable`:boolean*, `interval`:integer*, `powermode`:string*never/sleep/standby/idle, `tempdiff`:integer*, `tempmax`:integer* |
| `getDeviceSettings` | read | _(无参/无 schema)_ |
| `setDeviceSettings` | write | `uuid`:string, `devicefile`:string*, `enable`:boolean* |
| `getAttributes` | read | _(无参/无 schema)_ |
| `getSelfTestLogs` | read | _(无参/无 schema)_ |
| `getInformation` | read | _(无参/无 schema)_ |
| `getExtendedInformation` | read | _(无参/无 schema)_ |
| `getScheduleList` | read | _(无参/无 schema)_ |
| `getScheduledTest` | read | _(无参/无 schema)_ |
| `setScheduledTest` | write | `uuid`:string*, `enable`:boolean*, `devicefile`:string*, `comment`:string*, `type`:string*S/L/C/O, `hour`:string*, `month`:string*, `dayofmonth`:string*, `dayofweek`:string* |
| `deleteScheduledTest` | destructive | _(无参/无 schema)_ |
| `executeScheduledTest` | write | _(无参/无 schema)_ |

## Smb (`smb`) - 8 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getSettings` | read | _(无参/无 schema)_ |
| `setSettings` | write | `enable`:boolean*, `workgroup`:string*, `serverstring`:string*, `loglevel`:integer*0/1/2/3/10, `usesendfile`:boolean*, `aio`:boolean*, `timeserver`:boolean*, `winssupport`:boolean*, `winsserver`:string*, `homesenable`:boolean*, `homesbrowseable`:boolean*, `extraoptions`:string* |
| `getShareList` | read | _(无参/无 schema)_ |
| `getShare` | read | _(无参/无 schema)_ |
| `setShare` | write | `uuid`:string*, `enable`:boolean*, `sharedfolderref`:string*, `comment`:string*, `guest`:string*no/allow/only, `readonly`:boolean*, `browseable`:boolean*, `recyclebin`:boolean*, `recyclemaxsize`:integer*, `recyclemaxage`:integer*, `hidedotfiles`:boolean*, `inheritacls`:boolean*, `inheritpermissions`:boolean*, `easupport`:boolean*, `storedosattributes`:boolean*, `hostsallow`:string*, `hostsdeny`:string*, `audit`:boolean*, `timemachine`:boolean, `extraoptions`:string* |
| `deleteShare` | destructive | _(无参/无 schema)_ |
| `emptyRecycleBin` | destructive | _(无参/无 schema)_ |
| `getStats` | read | _(无参/无 schema)_ |

## Ssh (`ssh`) - 3 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `get` | read | _(无参/无 schema)_ |
| `set` | write | `enable`:boolean*, `port`:integer*, `passwordauthentication`:boolean*, `pubkeyauthentication`:boolean*, `permitrootlogin`:boolean*, `tcpforwarding`:boolean*, `compression`:boolean*, `extraoptions`:string* |
| `getStats` | read | _(无参/无 schema)_ |

## Syslog (`syslog`) - 2 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getSettings` | read | _(无参/无 schema)_ |
| `setSettings` | write | `enable`:boolean*, `port`:integer*, `host`:string*, `protocol`:string*udp/tcp |

## System (`system`) - 16 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `noop` | write | _(无参/无 schema)_ |
| `getTopInfo` | read | `format`:stringtext/json |
| `getShells` | read | _(无参/无 schema)_ |
| `reboot` | write | `delay`:integer |
| `shutdown` | write | `delay`:integer |
| `standby` | write | `delay`:integer |
| `suspend` | write | _(无参/无 schema)_ |
| `hibernate` | write | _(无参/无 schema)_ |
| `getTimeSettings` | read | _(无参/无 schema)_ |
| `setTimeSettings` | write | `timezone`:string*, `ntpenable`:boolean*, `ntptimeservers`:string*, `ntpclients`:string |
| `setDate` | write | `timestamp`:integer* |
| `getTimeZoneList` | read | _(无参/无 schema)_ |
| `getInformation` | read | _(无参/无 schema)_ |
| `getDiagnosticReport` | read | _(无参/无 schema)_ |
| `getCpuStats` | read | _(无参/无 schema)_ |
| `getCpuGovernors` | read | _(无参/无 schema)_ |

## Usermgmt (`usermgmt`) - 23 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `authUser` | write | `username`:string*, `password`:string* |
| `verifyChallenge` | write | `username`:string*, `challengeresponse`:any, `challengedata`:any |
| `enumerateSystemUsers` | read | _(无参/无 schema)_ |
| `enumerateUsers` | read | _(无参/无 schema)_ |
| `enumerateAllUsers` | read | _(无参/无 schema)_ |
| `enumerateSystemGroups` | read | _(无参/无 schema)_ |
| `enumerateGroups` | read | _(无参/无 schema)_ |
| `enumerateAllGroups` | read | _(无参/无 schema)_ |
| `getUserList` | read | `start`:integer*, `limit`:['integer', 'null']*, `sortfield`:['string', 'null'], `sortdir`:['string', 'null']asc/ASC/desc/DESC, `search`:['string', 'integer', 'null'], `detail`:stringbasic/full |
| `getUser` | read | `name`:string* |
| `getUserByContext` | read | _(无参/无 schema)_ |
| `setUser` | write | `name`:string*, `uid`:integer, `groups`:array*, `shell`:string, `password`:string*, `email`:string*, `comment`:string, `disallowusermod`:boolean*, `sshpubkeys`:array* |
| `setUserByContext` | write | `password`:string, `email`:string*, `comment`:string* |
| `deleteUser` | destructive | `name`:string* |
| `importUsers` | write | _(无参/无 schema)_ |
| `getGroupList` | read | _(无参/无 schema)_ |
| `getGroup` | read | `name`:string* |
| `setGroup` | write | `name`:string*, `gid`:integer, `comment`:string*, `members`:array* |
| `deleteGroup` | destructive | `name`:string* |
| `importGroups` | write | _(无参/无 schema)_ |
| `getSettings` | read | _(无参/无 schema)_ |
| `setSettings` | write | `enable`:boolean*, `sharedfolderref`:string* |
| `setPasswordByContext` | write | `password`:string* |

## Webgui (`webgui`) - 7 方法

| 方法 | 分类 | 参数 |
|---|---|---|
| `getSettings` | read | _(无参/无 schema)_ |
| `setSettings` | write | `port`:integer*, `enablessl`:boolean*, `sslport`:integer*, `forcesslonly`:boolean*, `sslcertificateref`:string*, `timeout`:integer* |
| `dismissWelcomeMessage` | write | _(无参/无 schema)_ |
| `getLocalStorageItems` | read | `devicetype`:string*desktop/mobile |
| `setLocalStorageItem` | write | `devicetype`:string*desktop/mobile, `key`:string*, `value`:any* |
| `clearLocalStorageItems` | write | `devicetype`:string*desktop/mobile |
| `getAutoLogoutSettings` | read | _(无参/无 schema)_ |

## 统计
- read(只读,自动放行): **142**
- write(改,需审计): **99**
- destructive(破坏性,必须用户确认): **24**
- 无参/无 schema: 157