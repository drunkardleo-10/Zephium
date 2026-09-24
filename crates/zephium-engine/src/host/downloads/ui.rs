use super::*;

impl Downloads {
    pub(crate) fn call(
        self: &Rc<Self>,
        partition: Partition,
        call: DownloadCall,
        done: DownloadCompletion,
    ) {
        if self.stopping.get()
            || self.retired.borrow().contains(&partition.profile())
            || !call.validate()
        {
            done.finish(DownloadResponse::Error {
                error: DownloadError::Unavailable,
            });
            return;
        }
        if matches!(call, DownloadCall::Updates) {
            let mut records: Vec<_> = self
                .recent
                .borrow()
                .iter()
                .filter(|(owner, _)| owner.profile() == partition.profile())
                .map(|(_, record)| record.view())
                .collect();
            records.extend(
                self.active
                    .borrow()
                    .values()
                    .filter(|transfer| transfer.partition.profile() == partition.profile())
                    .map(|transfer| transfer.record.view()),
            );
            let removed = self
                .forgotten
                .borrow()
                .iter()
                .filter(|(owner, _)| *owner == partition.profile())
                .map(|(_, id)| id.to_string())
                .collect();
            done.finish(DownloadResponse::Updates {
                entries: records,
                removed,
            });
            return;
        }
        if self.calls.borrow().len() >= MAX_UI_CALLS {
            done.finish(DownloadResponse::Error {
                error: DownloadError::Capacity,
            });
            return;
        }
        if let DownloadCall::Cancel { id } = &call {
            let id = DownloadId::parse(id).expect("validated download id");
            let can_cancel = self.active.borrow().get(&id).is_some_and(|transfer| {
                transfer.partition.profile() == partition.profile()
                    && !transfer.record.state.terminal()
                    && transfer.record.state != DownloadState::Finalizing
            });
            if can_cancel {
                self.cancel(id, None);
                done.finish(DownloadResponse::Accepted);
            } else {
                done.finish(DownloadResponse::Error {
                    error: DownloadError::Invalid,
                });
            }
            return;
        }
        let token = self.next_call.get();
        let Some(next) = token.checked_add(1) else {
            done.finish(DownloadResponse::Error {
                error: DownloadError::Capacity,
            });
            return;
        };
        self.next_call.set(next);
        self.calls.borrow_mut().insert(
            token,
            UiCall {
                partition,
                call: call.clone(),
                done,
            },
        );
        self.ensure_timer();
        let private = matches!(partition, Partition::Ephemeral(_));
        match call {
            DownloadCall::List { before, limit } => {
                if private {
                    self.ui_reply(token, DownloadStoreReply::Page(Vec::new()));
                } else {
                    self.ui_store(
                        token,
                        partition,
                        DownloadStoreCall::List {
                            before: before.and_then(|id| DownloadId::parse(&id)),
                            limit,
                            session: self.session,
                            active: self
                                .active
                                .borrow()
                                .iter()
                                .filter(|(_, transfer)| {
                                    transfer.partition.profile() == partition.profile()
                                })
                                .map(|(id, _)| *id)
                                .collect(),
                        },
                    );
                }
            }
            DownloadCall::SetAskDestination { enabled } => {
                if let Some(request) = self.calls.borrow_mut().get_mut(&token) {
                    request.call = DownloadCall::Preferences;
                }
                if private {
                    let mut preferences = self
                        .preferences
                        .borrow()
                        .get(&partition.profile())
                        .cloned()
                        .unwrap_or_default();
                    preferences.ask_destination = enabled;
                    self.ui_reply(token, DownloadStoreReply::Preferences(preferences));
                } else {
                    self.ui_store(
                        token,
                        partition,
                        DownloadStoreCall::SetPreferences(
                            DownloadPreferenceChange::AskDestination(enabled),
                        ),
                    );
                }
            }
            DownloadCall::Preferences | DownloadCall::ChooseDirectory => {
                let cached = self.preferences.borrow().get(&partition.profile()).cloned();
                if private {
                    self.ui_reply(
                        token,
                        DownloadStoreReply::Preferences(cached.unwrap_or_default()),
                    );
                } else {
                    self.ui_store(token, partition, DownloadStoreCall::Preferences);
                }
            }
            DownloadCall::Open { id } | DownloadCall::Reveal { id } => {
                let id = DownloadId::parse(&id).expect("validated download id");
                let recent = self
                    .recent
                    .borrow()
                    .iter()
                    .find(|(owner, record)| {
                        owner.profile() == partition.profile() && record.id == id
                    })
                    .map(|(_, record)| record.clone());
                if let Some(record) = recent {
                    self.ui_reply(token, DownloadStoreReply::Record(Some(Box::new(record))));
                } else if private {
                    self.ui_reply(token, DownloadStoreReply::Error(DownloadError::MissingFile));
                } else {
                    self.ui_store(token, partition, DownloadStoreCall::Get(id));
                }
            }
            DownloadCall::Forget { id } => {
                let id = DownloadId::parse(&id).expect("validated download id");
                if self.active.borrow().contains_key(&id) {
                    self.ui_reply(token, DownloadStoreReply::Error(DownloadError::Invalid));
                } else if private {
                    self.ui_reply(token, DownloadStoreReply::Saved);
                } else {
                    self.ui_store(token, partition, DownloadStoreCall::Forget(id));
                }
            }
            DownloadCall::Cancel { .. } | DownloadCall::Updates => unreachable!(),
        }
    }

    fn ui_store(&self, token: u64, partition: Partition, call: DownloadStoreCall) {
        self.work.set(self.work.get() + 1);
        let sender = self.sender.clone();
        if !self.store.download_call(
            partition.profile(),
            call,
            Box::new(move |reply| {
                let _ = sender.send(Message::Ui(token, reply));
            }),
        ) {
            let _ = self.sender.send(Message::Ui(
                token,
                DownloadStoreReply::Error(DownloadError::Storage),
            ));
        }
    }

    pub(super) fn ui_reply(self: &Rc<Self>, token: u64, reply: DownloadStoreReply) {
        let Some(request) = self.calls.borrow_mut().remove(&token) else {
            return;
        };
        let UiCall {
            partition,
            call,
            done,
        } = request;
        if let DownloadStoreReply::Error(error) = reply {
            done.finish(DownloadResponse::Error { error });
            return;
        }
        match (call, reply) {
            (DownloadCall::List { before, limit }, DownloadStoreReply::Page(mut records)) => {
                self.recover_page(partition, &records);
                let full = records.len() == limit as usize;
                for (owner, record) in self.recent.borrow().iter() {
                    if owner.profile() == partition.profile() {
                        records.retain(|old| old.id != record.id);
                        records.push(record.clone());
                    }
                }
                for transfer in self
                    .active
                    .borrow()
                    .values()
                    .filter(|transfer| transfer.partition.profile() == partition.profile())
                {
                    records.retain(|record| record.id != transfer.record.id);
                    records.push(transfer.record.clone());
                }
                records.retain(|record| {
                    before
                        .as_ref()
                        .is_none_or(|before| record.id.to_string() < *before)
                });
                records.sort_by_key(|record| std::cmp::Reverse(record.id));
                let more = full || records.len() > limit as usize;
                records.truncate(limit as usize);
                let next = if more {
                    records.last().map(|record| record.id.to_string())
                } else {
                    None
                };
                done.finish(DownloadResponse::Page {
                    entries: records.iter().map(DownloadRecord::view).collect(),
                    next,
                    supported: true,
                });
            }
            (DownloadCall::Preferences, DownloadStoreReply::Preferences(preferences)) => {
                self.preferences
                    .borrow_mut()
                    .insert(partition.profile(), preferences.clone());
                done.finish(DownloadResponse::Preferences {
                    preferences,
                    supported: true,
                });
            }
            (DownloadCall::ChooseDirectory, DownloadStoreReply::Preferences(preferences)) => {
                self.choose_directory(token, partition, preferences, done)
            }
            (
                call @ (DownloadCall::Open { .. } | DownloadCall::Reveal { .. }),
                DownloadStoreReply::Record(Some(record)),
            ) => {
                if record.state != DownloadState::Completed {
                    done.finish(DownloadResponse::Error {
                        error: DownloadError::Invalid,
                    });
                    return;
                }
                let (Some(path), Some(identity)) = (record.destination, record.identity) else {
                    done.finish(DownloadResponse::Error {
                        error: DownloadError::MissingFile,
                    });
                    return;
                };
                self.calls.borrow_mut().insert(
                    token,
                    UiCall {
                        partition,
                        call,
                        done,
                    },
                );
                self.work.set(self.work.get() + 1);
                let sender = self.sender.clone();
                if std::thread::Builder::new()
                    .name("zephium-download-file-check".into())
                    .spawn(move || {
                        let result = verify_file(&path, &identity);
                        let _ = sender.send(Message::Verified(token, path, result));
                    })
                    .is_err()
                {
                    self.work.set(self.work.get() - 1);
                    self.ui_reply(token, DownloadStoreReply::Error(DownloadError::Unavailable));
                }
            }
            (DownloadCall::Forget { id }, DownloadStoreReply::Saved) => {
                self.recent.borrow_mut().retain(|(owner, record)| {
                    owner.profile() != partition.profile() || record.id.to_string() != id
                });
                if let Some(id) = DownloadId::parse(&id) {
                    let mut forgotten = self.forgotten.borrow_mut();
                    forgotten.push_front((partition.profile(), id));
                    while forgotten.len() > RECENT_LIMIT {
                        forgotten.pop_back();
                    }
                }
                (self.notify)(partition.profile());
                done.finish(DownloadResponse::Applied);
            }
            (_, DownloadStoreReply::Record(None)) => done.finish(DownloadResponse::Error {
                error: DownloadError::MissingFile,
            }),
            _ => done.finish(DownloadResponse::Error {
                error: DownloadError::Unavailable,
            }),
        }
    }

    pub(super) fn verified(&self, token: u64, path: PathBuf, result: Result<(), DownloadError>) {
        let Some(request) = self.calls.borrow_mut().remove(&token) else {
            return;
        };
        if let Err(error) = result {
            request.done.finish(DownloadResponse::Error { error });
            return;
        }
        let Some(path) = path.to_str() else {
            request.done.finish(DownloadResponse::Error {
                error: DownloadError::Destination,
            });
            return;
        };
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let workspace = NSWorkspace::sharedWorkspace();
        match request.call {
            DownloadCall::Reveal { .. } => {
                workspace.activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
                request.done.finish(DownloadResponse::Applied);
            }
            DownloadCall::Open { .. } => {
                request.done.finish(if workspace.openURL(&url) {
                    DownloadResponse::Applied
                } else {
                    DownloadResponse::Error {
                        error: DownloadError::Unavailable,
                    }
                });
            }
            _ => request.done.finish(DownloadResponse::Error {
                error: DownloadError::Invalid,
            }),
        }
    }

    pub(super) fn finish_directory_selection(
        self: &Rc<Self>,
        request: UiCall,
        preferences: DownloadPreferences,
    ) {
        let token = self.next_call.get();
        let Some(next) = token.checked_add(1) else {
            request.done.finish(DownloadResponse::Error {
                error: DownloadError::Capacity,
            });
            return;
        };
        self.next_call.set(next);
        self.save_preferences(token, request.partition, preferences, request.done);
    }

    fn save_preferences(
        self: &Rc<Self>,
        token: u64,
        partition: Partition,
        preferences: DownloadPreferences,
        done: DownloadCompletion,
    ) {
        if matches!(partition, Partition::Ephemeral(_)) {
            let mut current = self
                .preferences
                .borrow()
                .get(&partition.profile())
                .cloned()
                .unwrap_or_default();
            current.directory = preferences.directory;
            current.directory_identity = preferences.directory_identity;
            let preferences = current;
            self.preferences
                .borrow_mut()
                .insert(partition.profile(), preferences.clone());
            done.finish(DownloadResponse::Preferences {
                preferences,
                supported: true,
            });
            return;
        }
        self.calls.borrow_mut().insert(
            token,
            UiCall {
                partition,
                call: DownloadCall::Preferences,
                done,
            },
        );
        let (Some(path), Some(identity)) = (preferences.directory, preferences.directory_identity)
        else {
            self.ui_reply(token, DownloadStoreReply::Error(DownloadError::Invalid));
            return;
        };
        self.ui_store(
            token,
            partition,
            DownloadStoreCall::SetPreferences(DownloadPreferenceChange::Directory {
                path,
                identity,
            }),
        );
    }

    fn choose_directory(
        self: &Rc<Self>,
        token: u64,
        partition: Partition,
        preferences: DownloadPreferences,
        done: DownloadCompletion,
    ) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(window) = objc2_app_kit::NSApplication::sharedApplication(mtm).keyWindow() else {
            return;
        };
        if window.attachedSheet().is_some() {
            done.finish(DownloadResponse::Error {
                error: DownloadError::Capacity,
            });
            return;
        }
        let Some(lease) = PanelLease::acquire() else {
            done.finish(DownloadResponse::Error {
                error: DownloadError::Capacity,
            });
            return;
        };
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseDirectories(true);
        panel.setCanChooseFiles(false);
        panel.setAllowsMultipleSelection(false);
        panel.setMessage(Some(&NSString::from_str(
            "Choose where Zephium saves downloads",
        )));
        if self.stopping.get()
            || self.retired.borrow().contains(&partition.profile())
            || !window.isVisible()
            || !window.isKeyWindow()
            || window.attachedSheet().is_some()
        {
            done.finish(DownloadResponse::Error {
                error: DownloadError::Unavailable,
            });
            return;
        }
        self.calls.borrow_mut().insert(
            token,
            UiCall {
                partition,
                call: DownloadCall::ChooseDirectory,
                done,
            },
        );
        self.directory_panels
            .borrow_mut()
            .insert(token, (panel.clone(), lease));
        let selected = panel.clone();
        let weak = Rc::downgrade(self);
        let callback = RcBlock::new(move |result: NSModalResponse| {
            let Some(manager) = weak.upgrade() else {
                return;
            };
            let _panel_owner = manager.directory_panels.borrow_mut().remove(&token);
            let request = manager.calls.borrow_mut().remove(&token);
            let Some(request) = request else { return };
            let path = (result == NSModalResponseOK)
                .then(|| selected.URLs())
                .and_then(|urls| urls.firstObject())
                .and_then(|url| url.path())
                .and_then(|path| bounded(&path, 4096));
            selected.orderOut(None);
            if let Some(path) = path {
                manager.work.set(manager.work.get() + 1);
                let sender = manager.sender.clone();
                let preferences = preferences.clone();
                if std::thread::Builder::new()
                    .name("zephium-download-directory".into())
                    .spawn(move || {
                        let result =
                            super::super::download_files::select_directory(PathBuf::from(path));
                        let _ =
                            sender.send(Message::DirectorySelected(request, preferences, result));
                    })
                    .is_err()
                {
                    manager.work.set(manager.work.get() - 1);
                }
            } else {
                request.done.finish(DownloadResponse::Error {
                    error: DownloadError::Cancelled,
                });
            }
        });
        panel.beginSheetModalForWindow_completionHandler(&window, &callback);
    }
}

impl Downloads {
    fn recover_page(&self, partition: Partition, records: &[DownloadRecord]) {
        if matches!(partition, Partition::Ephemeral(_)) || self.recovering.get() {
            return;
        }
        let records: Vec<_> = records
            .iter()
            .filter(|record| {
                record.state.terminal()
                    && record.staging.is_some()
                    && record.staging_identity.is_some()
                    && !self.recovered.borrow().contains(&record.id)
            })
            .cloned()
            .collect();
        if records.is_empty() {
            return;
        }
        self.recovering.set(true);
        self.work.set(self.work.get() + 1);
        let sender = self.sender.clone();
        if std::thread::Builder::new()
            .name("zephium-download-recovery".into())
            .spawn(move || {
                let cleaned = records
                    .into_iter()
                    .filter_map(|record| {
                        super::super::download_files::recover_staging(&record).ok()?;
                        Some((record.id, record.staging_identity?))
                    })
                    .collect();
                let _ = sender.send(Message::Recovered(partition.profile(), cleaned));
            })
            .is_err()
        {
            self.recovering.set(false);
            self.work.set(self.work.get() - 1);
        }
    }
}
